# cratestack-auth

Signed-request and identity-token auth for CrateStack applications: SigV4-style
canonical-request signing/verification over Ed25519, SD-JWT id-token
issuance/verification, multi-issuer JWKS resolution, per-service signing
identities, and COSE-signed enrolment challenges.

## Overview

- [`sign_request`] / [`SignedRequestVerifier`] — build and verify an
  `Authorization: Signature keyId="...", timestamp="...", nonce="...",
  signature="..."` header over a canonical form of the request (method, path,
  sorted query, body hash, timestamp, nonce, key id), with replay protection
  ([`NonceStore`], with an in-memory and a Redis-backed implementation) and
  an optional multi-issuer JWKS fallback for key rotation without an env-var
  redeploy.
- [`issue_sd_id_token`] / [`IdTokenVerifier`] — mint and verify an SD-JWT
  (`<jwt>~disclosure1~...~`), with selective disclosure so a holder reveals
  only the claims a given verifier needs.
- [`ServiceSigningKey`] / [`MultiIssuerJwksVerifier`] — a per-service Ed25519
  signing identity (`from_env`/`ephemeral`) plus a trust-list-driven verifier
  that fetches and caches JWKS per trusted issuer.
- [`build_cose_enroll_response`] / [`parse_cose_enroll_response`] — COSE
  `Sign1`-signed enrolment challenge responses.
- [`SignedRequestAuthProvider`] — a `cratestack_core::AuthProvider`
  implementation wiring [`SignedRequestVerifier`] into a cratestack server's
  auth pipeline.

## The `axum` feature

Default-on. Gates the three items that touch the `axum` crate itself:
[`require_signed_request`] (a tower auth middleware), [`jwks_router`] (a
mountable `axum::Router` serving `/jwks.json` and `/.well-known/jwks.json`),
and the `FromRequestParts` extractor impls on `CurrentPrincipal`/
`AuthenticatedPrincipal`. Everything else — signing, verifying,
`SignedRequestAuthProvider`, SD-JWT issuance/verification,
`ServiceSigningKey`, `MultiIssuerJwksVerifier`, COSE challenges — needs only
the plain `http` crate for header/method/URI types, not `axum`, so a
signing-only consumer (a `cratestack-client` SDK, say) can:

```toml
[dependencies]
cratestack-auth = { version = "0.7", default-features = false }
```

and keep axum (and its own tower/hyper/matchit dependency tree) out of its
build entirely.

## Installation

```toml
[dependencies]
cratestack-auth = "0.7"
```

## Usage

Signing a request as a client:

```rust,no_run
use cratestack_auth::{SignRequestParams, sign_request};
use ed25519_dalek::SigningKey;
use http::Method;

# fn example(signing_key: &SigningKey, timestamp: &str, body: &[u8]) {
let signature = sign_request(SignRequestParams {
    signing_key,
    method: &Method::POST,
    path: "/uploads/presign",
    query: None,
    body,
    timestamp,
    nonce: "n_123",
    key_id: "vk_123",
});
let header = format!(
    "Signature keyId=\"vk_123\", timestamp=\"{timestamp}\", nonce=\"n_123\", signature=\"{signature}\""
);
# let _ = header;
# }
```

Verifying it as a server, mounted as axum middleware:

```rust,no_run
use axum::{Router, middleware, routing::post};
use cratestack_auth::{SignedRequestVerifier, require_signed_request};

# async fn handler() -> &'static str { "ok" }
# fn example(verifier: SignedRequestVerifier) -> Router {
Router::new()
    .route("/uploads/presign", post(handler))
    .layer(middleware::from_fn_with_state(verifier, require_signed_request))
# }
```

Loading a per-service signing identity and serving its JWKS:

```rust,no_run
use axum::Router;
use cratestack_auth::{ServiceSigningKey, jwks_router};

# fn example() -> Result<Router, Box<dyn std::error::Error>> {
let signing_key = ServiceSigningKey::from_env(
    "vendor-service",
    "vendor-service-v1",
    "VENDOR_SERVICE_SIGNING_KEY",
)?;
let app = Router::new().merge(jwks_router(signing_key.jwks_document()));
# Ok(app)
# }
```

## Environment variables

| Variable | Used by |
| --- | --- |
| `CRATESTACK_AUTH_CHALLENGE_SIGNING_KEY` | [`build_cose_enroll_response`]/[`parse_cose_enroll_response`] — **required**, fails closed if absent, empty, or whitespace-only. |
| `CRATESTACK_AUTH_SIGNATURE_TRUSTED_KEYS` | [`SignedRequestVerifier::from_env`] — `keyId:base64url,...` static trust list. |
| `CRATESTACK_AUTH_SIGNATURE_TRUSTED_ISSUERS` | [`SignedRequestVerifier::from_env`] — JSON `{issuer: jwks_url}` map. |
| `CRATESTACK_AUTH_SIGNATURE_MAX_SKEW_SECONDS` | [`SignedRequestVerifier::from_env`] — default 300. |
| `CRATESTACK_AUTH_SIGNATURE_REPLAY_WINDOW_SECONDS` | [`SignedRequestVerifier::from_env`] — default 300. |
| `CRATESTACK_AUTH_ID_TOKEN_AUDIENCE` | [`IdTokenVerifier::audience_from_env`] — optional, no fail-closed contract. |

A per-service signing key (`ServiceSigningKey::from_env`) and the
`CHALLENGE_SIGNING_KEY_ENV` seed use the same encoding: URL-safe-base64-no-pad
of the raw 32-byte Ed25519 seed (see [`encode_signing_key`]/
[`decode_signing_key`]).

## Security note

[`CHALLENGE_SIGNING_KEY_ENV`]'s doc comment records why this fails closed
rather than falling back to any default: a hardcoded seed literal is
permanently compromised the moment it reaches git history, forks, or CI
logs. Never reuse a committed literal as a real signing key, including in
tests — this crate's own tests generate their own key material.

## See Also

- `cratestack-service` — sibling absorption; deliberately did **not**
  extend `ServiceConfig` with `issuer_url`/`jwks_url`, flagging them as
  this crate's concern (`MultiIssuerJwksVerifier`'s multi-issuer map is a
  different shape than a single-issuer config field pair could represent).
- `cratestack-core` — `AuthProvider`, `CratestackContext`, `RequestContext`,
  `CratestackError` — the trait/types [`SignedRequestAuthProvider`] implements
  against.

## License

MIT
