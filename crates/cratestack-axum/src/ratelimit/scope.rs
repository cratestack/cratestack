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

/// The address form a per-peer budget counts against.
///
/// IPv6 is aggregated to its **/64** because that is the smallest block
/// routinely delegated to a single subscriber: without aggregation an
/// attacker with one ordinary residential prefix has 2^64 distinct "peers"
/// and the per-peer cap costs them nothing. IPv4 is deliberately NOT
/// aggregated — /24 collateral under CGNAT would collapse thousands of
/// unrelated subscribers into one budget, and IPv4 gives an attacker no
/// comparable free-address supply.
pub(super) fn scope_address(ip: IpAddr) -> String {
    match ip {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => {
            let s = v6.segments();
            let network = Ipv6Addr::new(s[0], s[1], s[2], s[3], 0, 0, 0, 0);
            format!("{network}/64")
        }
    }
}
