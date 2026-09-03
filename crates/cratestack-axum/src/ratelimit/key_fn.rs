//! The default bucket-key derivation, and the default "should this
//! request be rate-limited at all" filter.
//!
//! Split out of `layer.rs` verbatim (cratestack#846) to keep that file
//! under the workspace's 200-line ceiling once the store-error policy
//! landed there. cratestack#871 then changed what it *returns* — a key
//! plus, when that key is caller-mintable, the [`BucketBudget`] governing
//! how many such keys its scope may create. The cratestack#416
//! fail-closed rationale below is unchanged.

use std::net::SocketAddr;
use std::sync::Once;

use axum::extract::{ConnectInfo, Request};
use cratestack_core::{BucketBudget, CratestackError};
use http::header;
use sha2::{Digest, Sha256};

use super::budget::RateLimitBucketBudget;
use super::budget::warn::BudgetWarnings;
use super::scope::{
    BudgetScope, KeyDerivation, UnverifiedAuthPolicy, VerifiedPrincipal, scope_address,
};

/// Logged once per process, not per request — see the identical rationale
/// in `idempotency::layer::MISSING_IDENTITY_WARNING`.
static MISSING_IDENTITY_WARNING: Once = Once::new();

/// cratestack#416: the pre-existing default silently collapsed every
/// unauthenticated caller without a verified peer address onto a single
/// shared `"anonymous"` rate-limit bucket — no per-caller throttling at all
/// for that traffic, and one caller could exhaust another's budget. Refusing
/// the request instead makes the gap loud in staging/CI rather than a
/// silently-reachable production bypass.
///
/// cratestack#871: the `auth:` branch below still keys on an **unverified**
/// header — this layer runs before authentication — so it is now handed a
/// budget instead of being trusted to be low-cardinality. The key *shape*
/// is unchanged, so no existing bucket moves; what changes is that the
/// scope which mints it can only mint so many.
pub(super) fn default_key_fn(
    req: &Request,
    budget: RateLimitBucketBudget,
    policy: UnverifiedAuthPolicy,
    warnings: &BudgetWarnings,
) -> Result<KeyDerivation, CratestackError> {
    // A principal an upstream layer actually verified is not caller-
    // mintable, so it needs no budget: its cardinality is bounded by the
    // number of principals that exist. Hashed like the header below so an
    // identifier never lands in a store key verbatim.
    if let Some(VerifiedPrincipal(principal)) = req.extensions().get::<VerifiedPrincipal>() {
        return Ok(KeyDerivation::unbudgeted(format!(
            "princ:{}",
            sha256_hex(principal.as_bytes())
        )));
    }

    let peer = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(addr)| addr.ip());

    // Prefer Authorization header for authenticated requests.
    if policy == UnverifiedAuthPolicy::Budget
        && let Some(auth_header) = req.headers().get(header::AUTHORIZATION)
        && let Ok(auth_str) = auth_header.to_str()
    {
        let key = format!("auth:{}", sha256_hex(auth_str.as_bytes()));
        return Ok(match peer {
            Some(ip) => KeyDerivation::budgeted(
                key,
                BucketBudget::new(
                    format!("peer:{}", scope_address(ip)),
                    format!("ip:{ip}"),
                    budget.max_distinct_per_peer,
                    budget.window,
                ),
                BudgetScope::Peer,
            ),
            // No verified peer to attribute the cardinality to. The
            // header is still the best available *throttling* key (it
            // separates real callers), so it is kept — but every such
            // caller shares one cardinality budget, because there is
            // nothing to tell them apart by that they do not control.
            None => {
                warnings.missing_peer();
                KeyDerivation::budgeted(
                    key,
                    BucketBudget::new(
                        "global",
                        "overflow",
                        budget.max_distinct_global,
                        budget.window,
                    ),
                    BudgetScope::Global,
                )
            }
        });
    }

    // Fall back to the real TCP peer address for unauthenticated requests, to
    // avoid collisions between distinct callers. This is deliberately *not*
    // `Forwarded`/`X-Forwarded-For`: those headers are client-supplied and
    // this crate has no trusted-proxy configuration to verify or strip them,
    // so trusting them here would let an attacker mint a fresh rate-limit
    // bucket on every request just by rotating the header value. `ConnectInfo`
    // is populated by axum from the actual accepted socket (when the server
    // is served via `into_make_service_with_connect_info::<SocketAddr>()`)
    // and cannot be spoofed by the client.
    //
    // This is also where `UnverifiedAuthPolicy::Ignore` lands: one bucket
    // per verified peer, nothing caller-supplied in the key at all, and
    // therefore no budget needed. Note the address is NOT aggregated here
    // even for IPv6 — aggregation belongs to the *scope* (see
    // `scope::scope_address`); aggregating the throttling key itself would
    // make one subscriber's /64 a shared bucket, which is cratestack#416's
    // collision all over again.
    if let Some(ip) = peer {
        return Ok(KeyDerivation::unbudgeted(format!("ip:{ip}")));
    }

    // Neither Authorization nor a verified peer address is available (e.g.
    // the server isn't wired through `into_make_service_with_connect_info`).
    // There is no unforgeable value left to key on, so refuse rather than
    // collapsing every such caller onto one shared bucket.
    MISSING_IDENTITY_WARNING.call_once(|| {
        tracing::warn!(
            target: "cratestack",
            cratestack_operation = "rate_limit",
            "RateLimitLayer's default key function has no Authorization header and no \
             ConnectInfo<SocketAddr> peer on this request, so it cannot verify caller identity. \
             Refusing the request rather than collapsing distinct callers onto a shared \
             \"anonymous\" bucket (cratestack#416) — wire \
             into_make_service_with_connect_info::<SocketAddr>() or supply \
             RateLimitLayer::with_key_fn(...) explicitly. Logged once per process; every \
             matching request is refused until this is fixed.",
        );
    });
    Err(CratestackError::PreconditionFailed(
        "rate limit: no verifiable caller identity (Authorization header or ConnectInfo peer) \
         is available for the default bucket key; the server must be served through \
         into_make_service_with_connect_info::<SocketAddr>() or configure an explicit key \
         function"
            .to_owned(),
    ))
}

/// sha2 0.11 / digest 0.11 return `hybrid_array::Array`, which (unlike
/// digest 0.10's `GenericArray`) implements no `LowerHex`. The byte-wise
/// `{:02x}` fold below is this repo's existing hex idiom
/// (`cratestack-core/src/transport.rs`) and is byte-for-byte what
/// `format!("{:x}", …)` produced — this string is persisted/keyed on, so
/// it must not change shape.
fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Default rate limit filter: always rate-limit. Fail closed.
/// Custom filters can check operation descriptors and return false for
/// operations marked `@no_rate_limit` or similar exemptions.
pub(super) fn default_should_rate_limit_fn(_req: &Request) -> bool {
    true
}
