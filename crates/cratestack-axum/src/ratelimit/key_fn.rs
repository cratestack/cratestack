//! The default bucket-key derivation, and the default "should this
//! request be rate-limited at all" filter.
//!
//! Split out of `layer.rs` verbatim (cratestack#846) to keep that file
//! under the workspace's 200-line ceiling once the store-error policy
//! landed there. No behavioural change: cratestack#416's fail-closed
//! rationale below moved unedited.

use std::net::SocketAddr;
use std::sync::Once;

use axum::extract::{ConnectInfo, Request};
use cratestack_core::CratestackError;
use http::header;
use sha2::{Digest, Sha256};

/// Logged once per process, not per request — see the identical rationale
/// in `idempotency::layer::MISSING_IDENTITY_WARNING`.
static MISSING_IDENTITY_WARNING: Once = Once::new();

/// cratestack#416: the pre-existing default silently collapsed every
/// unauthenticated caller without a verified peer address onto a single
/// shared `"anonymous"` rate-limit bucket — no per-caller throttling at all
/// for that traffic, and one caller could exhaust another's budget. Refusing
/// the request instead makes the gap loud in staging/CI rather than a
/// silently-reachable production bypass.
pub(super) fn default_key_fn(req: &Request) -> Result<String, CratestackError> {
    // Prefer Authorization header for authenticated requests.
    if let Some(auth_header) = req.headers().get(header::AUTHORIZATION)
        && let Ok(auth_str) = auth_header.to_str()
    {
        let mut h = Sha256::new();
        h.update(auth_str.as_bytes());
        // sha2 0.11 / digest 0.11 return `hybrid_array::Array`, which (unlike
        // digest 0.10's `GenericArray`) implements no `LowerHex`. The
        // byte-wise `{:02x}` fold below is this repo's existing hex idiom
        // (`cratestack-core/src/transport.rs`) and is byte-for-byte what
        // `format!("{:x}", …)` produced — this string is persisted/keyed on,
        // so it must not change shape.
        let hex: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
        return Ok(format!("auth:{hex}"));
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
    if let Some(ConnectInfo(addr)) = req.extensions().get::<ConnectInfo<SocketAddr>>() {
        return Ok(format!("ip:{}", addr.ip()));
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

/// Default rate limit filter: always rate-limit. Fail closed.
/// Custom filters can check operation descriptors and return false for
/// operations marked `@no_rate_limit` or similar exemptions.
pub(super) fn default_should_rate_limit_fn(_req: &Request) -> bool {
    true
}
