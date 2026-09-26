//! The idempotency layer's principal fingerprints: the default, and the
//! pre-cratestack#1006 one kept public for a migration.

use std::net::SocketAddr;
use std::sync::Once;

use axum::extract::{ConnectInfo, Request};
use cratestack_core::CratestackError;
use http::header;
use sha2::{Digest, Sha256};

use crate::ratelimit::VerifiedPrincipal;

/// Logged once per process, not per request — a busy misconfigured
/// deployment would otherwise emit this thousands of times a second. See
/// `default_principal_fingerprint` for the condition that fires it.
static MISSING_IDENTITY_WARNING: Once = Once::new();

/// cratestack#416: the pre-existing default silently collapsed every
/// unauthenticated caller without a verified peer address onto a single
/// shared `"anonymous"` idempotency namespace — two distinct callers reusing
/// an `Idempotency-Key` could then replay each other's response. Refusing
/// the request instead (`PreconditionFailed`, matching this crate's
/// established "handled error, not an unwind" shape) makes the gap loud in
/// staging/CI instead of silently reachable in production, per the
/// ticket's Expected Behavior: "construction requires an explicit
/// fingerprint function so the collision cannot be reached by accident."
pub(crate) fn default_principal_fingerprint(req: &Request) -> Result<String, CratestackError> {
    // A principal an upstream layer verified (the COSE envelope layer's
    // signer thumbprint, ADR 0006 §12, cratestack#1006) comes first, as it
    // does in the rate limiter's default key: it is not caller-mintable, and
    // a COSE-only client sends no `Authorization` header, so without this
    // it was refused with the 412 below. The `princ:` prefix keeps it out of
    // the other two namespaces (bare hex and an IP address never start with
    // it), and it is hashed so an identifier is never stored verbatim.
    if let Some(VerifiedPrincipal(principal)) = req.extensions().get::<VerifiedPrincipal>() {
        let digest = Sha256::digest(principal.as_bytes());
        let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        return Ok(format!("princ:{hex}"));
    }

    legacy_principal_fingerprint(req)
}

/// The default fingerprint as it was before cratestack#1006, which put a
/// `VerifiedPrincipal` (the envelope layer's signer) first: the SHA-256 hex
/// of the `Authorization` header, else the `ConnectInfo` peer address,
/// else a `412` (cratestack#416).
///
/// Public for one migration (API-review decision, 2026-09-26): a
/// deployment that already inserts `VerifiedPrincipal` (its own middleware,
/// or the envelope layer) moves to the `princ:` namespace on upgrade, so a
/// key in flight across the deploy would run again instead of replaying.
/// Install this with
/// [`IdempotencyLayer::with_legacy_principal_fingerprint`](super::IdempotencyLayer::with_legacy_principal_fingerprint)
/// for the deploy, and drop it once the idempotency TTL has passed.
pub fn legacy_principal_fingerprint(req: &Request) -> Result<String, CratestackError> {
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
        return Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect());
    }

    // Fall back to the real TCP peer address for unauthenticated requests, to
    // avoid collisions between distinct callers. This is deliberately *not*
    // `Forwarded`/`X-Forwarded-For`: those headers are client-supplied and
    // this crate has no trusted-proxy configuration to verify or strip them,
    // so trusting them here would let an attacker land in another caller's
    // idempotency namespace just by guessing/spoofing that caller's apparent
    // IP. `ConnectInfo` is populated by axum from the actual accepted socket
    // (when the server is served via `into_make_service_with_connect_info::<SocketAddr>()`)
    // and cannot be spoofed by the client.
    if let Some(ConnectInfo(addr)) = req.extensions().get::<ConnectInfo<SocketAddr>>() {
        return Ok(addr.ip().to_string());
    }

    // Neither Authorization nor a verified peer address is available (e.g.
    // the server isn't wired through `into_make_service_with_connect_info`).
    // There is no unforgeable value left to key on, so refuse rather than
    // collapsing every such caller onto one shared namespace.
    MISSING_IDENTITY_WARNING.call_once(|| {
        tracing::warn!(
            target: "cratestack",
            cratestack_operation = "idempotency",
            "IdempotencyLayer's default principal fingerprint has no Authorization header and \
             no ConnectInfo<SocketAddr> peer on this request, so it cannot verify caller \
             identity. Refusing the request rather than collapsing distinct callers onto a \
             shared \"anonymous\" namespace (cratestack#416) — wire \
             into_make_service_with_connect_info::<SocketAddr>() or supply \
             IdempotencyLayer::with_principal_fingerprint(...) explicitly. Logged once per \
             process; every matching request is refused until this is fixed.",
        );
    });
    Err(CratestackError::PreconditionFailed(
        "idempotency: no verifiable caller identity (Authorization header or ConnectInfo peer) \
         is available for the default namespace fingerprint; the server must be served through \
         into_make_service_with_connect_info::<SocketAddr>() or configure an explicit \
         fingerprint function"
            .to_owned(),
    ))
}
