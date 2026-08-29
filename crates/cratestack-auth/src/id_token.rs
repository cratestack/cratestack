//! SD-JWT id-token ("id_jwt") claims, issuance, JWKS-backed verification, and
//! the request-principal types/extractors built on top of a validated token.
//!
//! Split by concern across `id_token/*.rs` to stay under this repo's 200-LoC
//! file ceiling; each submodule owns one piece of the id_jwt lifecycle:
//! claim shapes ([`claims`]), the request-principal types ([`principal`]),
//! the axum `FromRequestParts` extractors ([`extractor`], `axum`-feature
//! gated), signature verification against a JWKS ([`verifier`]), minting
//! ([`issuance`]), Ed25519 <-> JWK conversion ([`jwk`]), SD-JWT disclosure
//! handling ([`disclosure`]), and compact-form parsing ([`token_parsing`]).
//! This module's public surface (re-exported below) is frozen: `lib.rs`,
//! `signed_request.rs`, and downstream crates depend on these exact names
//! resolving at `id_token::<Name>`.

mod claims;
mod disclosure;
#[cfg(feature = "axum")]
mod extractor;
mod issuance;
mod jwk;
mod principal;
mod token_parsing;
mod verifier;

#[cfg(test)]
mod tests;

pub use claims::{
    DEFAULT_ID_TOKEN_AUDIENCE, DisclosureClaim, ID_TOKEN_AUDIENCE_ENV, IdTokenClaims,
    IdTokenClaimsParams, IssuedSdIdToken,
};
pub use disclosure::decode_disclosures_unverified;
pub use issuance::{default_id_token_claims, issue_id_token, issue_sd_id_token, take_disclosures};
pub use jwk::{
    decode_signing_key, encode_signing_key, issuer_jwk, verifying_key_from_jwk, verifying_key_jwk,
};
pub use principal::{AuthenticatedPrincipal, CurrentPrincipal, RequestPrincipal, UserPrincipal};
pub use token_parsing::decode_id_token_claims_unverified;
pub use verifier::IdTokenVerifier;
