//! What the default key function produces, and the two knobs that change
//! which scope an unverified caller lands in (cratestack#871).

use std::net::{IpAddr, Ipv6Addr};

use cratestack_core::BucketBudget;

/// A caller identity an upstream layer has actually **verified**.
///
/// Insert it as a request extension from a layer that runs *before*
/// [`super::RateLimitLayer`] and has validated the credential (signature,
/// introspection, mTLS, session lookup). When present, the default key
/// function keys on it directly and applies **no** bucket budget: a
/// verified principal is not caller-mintable, so its cardinality is
/// bounded by however many principals actually exist.
///
/// This is opt-in rather than the default because in this framework
/// authentication runs *inside* the generated handlers — after this layer.
/// Making verified identity mandatory would collapse every existing
/// consumer's authenticated traffic onto the peer address overnight.
///
/// The inner value is hashed before it becomes a bucket key, so a
/// principal id is never written to a store key or a log line verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedPrincipal(pub String);

/// What to do with an `Authorization` header that **nothing has
/// verified** — which is every `Authorization` header this layer sees,
/// since it runs before authentication.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum UnverifiedAuthPolicy {
    /// Key on the (hashed) header as before, but subject to a
    /// [`BucketBudget`] that caps how many distinct buckets one scope may
    /// mint. Preserves per-caller throttling for real callers
    /// (cratestack#416) while bounding the keyspace an attacker can
    /// create. The default.
    #[default]
    Budget,
    /// Ignore the header entirely and key on the verified peer address.
    /// Strictly stronger against amplification — nothing caller-supplied
    /// enters the key at all — at the cost of collapsing every caller
    /// behind one NAT/proxy egress into one bucket. Choose it when the
    /// limiter is a security control and callers are known to be
    /// per-address.
    Ignore,
}

/// Which scope a derived budget belongs to. Not part of [`BucketBudget`]:
/// a store cannot tell a peer scope from the global one and does not need
/// to, so the distinction lives here — where it is decided — and is used
/// only to pick how loudly to log an over-cap charge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BudgetScope {
    /// Per verified peer address (IPv6 aggregated to its /64).
    Peer,
    /// One scope for the whole process, used when no verified peer
    /// address is available at all.
    Global,
}

/// The default key function's full answer: the bucket the caller asked
/// for, and — when that bucket is caller-mintable — the budget governing
/// whether it may be created.
#[derive(Debug, Clone)]
pub(super) struct KeyDerivation {
    pub(super) key: String,
    pub(super) budget: Option<BucketBudget>,
    pub(super) scope: Option<BudgetScope>,
}

impl KeyDerivation {
    /// A key nobody can mint at will: a verified principal, a verified
    /// peer address, or whatever a consumer's own `with_key_fn` returns.
    pub(super) fn unbudgeted(key: String) -> Self {
        Self {
            key,
            budget: None,
            scope: None,
        }
    }

    pub(super) fn budgeted(key: String, budget: BucketBudget, scope: BudgetScope) -> Self {
        Self {
            key,
            budget: Some(budget),
            scope: Some(scope),
        }
    }
}

/// The address form used **everywhere a peer address becomes a key** — the
/// budget scope, the `ip:` fallback bucket, and the `ip:` bucket an
/// unauthenticated request gets.
///
/// IPv6 is aggregated to its **/64** because that is the smallest block
/// routinely delegated to a single subscriber: without aggregation an
/// attacker with one ordinary residential prefix has 2^64 distinct "peers"
/// and the per-peer cap costs them nothing. IPv4 is deliberately NOT
/// aggregated — /24 collateral under CGNAT would collapse thousands of
/// unrelated subscribers into one budget, and IPv4 gives an attacker no
/// comparable free-address supply.
///
/// # Why it is ONE function and not two (cratestack#871 review, blocker 1)
///
/// The first cut aggregated only the scope and left the bucket keys on the
/// full address. That left the whole mechanism evadable from the other
/// side, and it was measured: rotating the source address inside a single
/// /64 produced 200 buckets with an `Authorization` header (cap 8) and 200
/// buckets, 200/200 allowed, with **no header at all** — the cratestack#846
/// signature with the address, rather than the token, as the rotating
/// variable. Aggregating the scope while leaving the key un-aggregated
/// bounds nothing.
///
/// The accepted cost, stated rather than hidden: two distinct hosts inside
/// one *routable* /64 share a throttling bucket. That is a real
/// cratestack#416 trade-off, taken because a /64 is one subscriber and an
/// attacker's 2^64-address supply is not a hypothetical.
///
/// # IPv4-mapped addresses are unwrapped FIRST (cratestack#871 round-2)
///
/// A dual-stack listener — `TcpListener::bind("[::]:0")`, the ordinary
/// Linux bind — delivers every IPv4 client as `::ffff:a.b.c.d`. Those have
/// all-zero top groups, so blindly taking the /64 mapped **every IPv4
/// client in the world onto `ip:::/64`**: measured, 200 distinct IPv4
/// clients collapsed into 1 bucket with 5 allowed. That is a
/// cratestack#416 collision of unlimited width and a one-client denial of
/// service against all IPv4 traffic — strictly worse than the evasion the
/// aggregation was added to close.
///
/// So a mapped address is unwrapped to its IPv4 form and then treated
/// exactly like any other IPv4 address: per-address, never aggregated.
///
/// **`to_ipv4_mapped`, deliberately not `to_ipv4`.** The latter also
/// accepts the deprecated IPv4-*compatible* form (`::a.b.c.d`, RFC 4291
/// §2.5.5.1), which means it maps `::1` to `0.0.0.1` and `::` to
/// `0.0.0.0` — conflating the IPv6 loopback and the unspecified address
/// with real IPv4 addresses. That trades one collision for another.
/// Instead, the whole all-zero `::/64` region (unspecified, loopback, and
/// both mapped/compatible forms) is exempted from aggregation below and
/// keyed on the full address. Nothing in that region is globally routable,
/// so it hands an attacker no address supply to rotate through.
pub(super) fn bucket_address(ip: IpAddr) -> String {
    let v6 = match ip {
        IpAddr::V4(v4) => return v4.to_string(),
        IpAddr::V6(v6) => v6,
    };
    if let Some(v4) = v6.to_ipv4_mapped() {
        return v4.to_string();
    }
    let s = v6.segments();
    if s[0] == 0 && s[1] == 0 && s[2] == 0 && s[3] == 0 {
        // The `::/64` special region — see the note above. Aggregating it
        // would merge unrelated special addresses into one bucket.
        return v6.to_string();
    }
    let network = Ipv6Addr::new(s[0], s[1], s[2], s[3], 0, 0, 0, 0);
    format!("{network}/64")
}
