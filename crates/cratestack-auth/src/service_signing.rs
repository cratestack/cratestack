//! Per-service signing identity + multi-issuer verification.
//!
//! ## Why this module exists
//!
//! A single-issuer setup gives one service (an identity/auth service, say)
//! the *only* signing identity in the system: it issues `id_token`s and
//! publishes its public key at `/jwks.json`. Every other service that needs
//! to authenticate a user fetches that JWKS to verify id-token signatures
//! (and the SD-JWT disclosures stitched onto them).
//!
//! A signed-upload-ticket pattern needs the same shape, but issued by the
//! *owning* service for an asset (a vendor/catalog/order service, say) so a
//! generic upload handler can stay domain-naive while still enforcing
//! per-asset ACLs. Rather than build a parallel "upload-tickets-only"
//! mechanism, this module generalises the single-issuer pattern so any
//! backend service can become a JWT issuer.
//!
//! Other near-term consumers of the same plumbing:
//!
//! * **s2s request signing rotation.** Receivers can JWKS-fetch the
//!   sender's verifying key by `kid` instead of carrying public keys
//!   in env config.
//! * **Partner webhook signing.** When delivery-gateway spins off as
//!   the opaque 3rd-party API, partners verify webhook signatures via
//!   `delivery-gateway./jwks.json`.
//! * **Cross-service async events.** Producers sign events with their
//!   service key; consumers verify via JWKS — same code path.
//! * **Per-service scoped tokens.** Anything narrow (a vendor-service
//!   "private preview" token, a catalog "fast-search" token, etc.)
//!   piggy-backs on the same signing identity.
//!
//! ## Surface
//!
//! * [`ServiceSigningKey`] — load-or-mint persistent Ed25519 identity
//!   for a service.
//! * [`mint_signed_token`] — generic `JWT(claims)` minter for any
//!   serde-serializable claim shape.
//! * [`MultiIssuerJwksVerifier`] — fetches + caches JWKS per trusted
//!   issuer, verifies signatures, returns deserialised claims.
//! * [`jwks_router`] — mountable axum router that serves a service's
//!   public key at `/jwks.json` (and `/.well-known/jwks.json` for
//!   discovery convention).
//!
//! Claim-shape-specific validation (audience, scope, expiry slack,
//! nonce consumption, ...) stays with the consumer — this module
//! only owns the signature/exp envelope.
//!
//! ## Module layout
//!
//! Split by concern to stay under this crate's 200-LoC file convention:
//! - [`signing_key`]: [`ServiceSigningKey`] — load/mint/access a
//!   service's own signing identity.
//! - [`minting`]: [`mint_signed_token`] + the fixed `JwtHeader` shape.
//! - [`verifier_types`]: [`MultiIssuerJwksVerifier`]'s data (trust
//!   list, JWKS cache) + construction, and [`VerifiedToken`].
//! - [`verifier_verify`]: the `verify()` signature/expiry/trust check.
//! - [`verifier_cache`]: JWKS fetch + per-`kid` cache lookup/refresh.
//! - [`router`]: [`jwks_router`], the axum-gated `/jwks.json` mount.
//! - [`jwt_parse`]: shared compact-JWT-form decoding helper.
mod jwt_parse;
mod minting;
#[cfg(feature = "axum")]
mod router;
mod signing_key;
mod verifier_cache;
mod verifier_types;
mod verifier_verify;

#[cfg(test)]
mod tests_fixtures;
#[cfg(all(test, feature = "axum"))]
mod tests_router;
#[cfg(test)]
mod tests_signing_key;
#[cfg(test)]
mod tests_verifier;

pub use minting::mint_signed_token;
#[cfg(feature = "axum")]
pub use router::jwks_router;
pub use signing_key::ServiceSigningKey;
pub use verifier_types::{MultiIssuerJwksVerifier, VerifiedToken};
