//! CrateStack signed-request + identity-token auth.
//!
//! - SigV4-style canonical request construction/signing/verification over
//!   Ed25519 ([`sign_request`], [`SignedRequestVerifier`]).
//! - SD-JWT id-token issuance and verification ([`issue_sd_id_token`],
//!   [`IdTokenVerifier`]).
//! - Multi-issuer JWKS resolution ([`MultiIssuerJwksVerifier`]) and
//!   per-service signing identities ([`ServiceSigningKey`]).
//! - COSE-signed enrolment challenges ([`build_cose_enroll_response`] /
//!   [`parse_cose_enroll_response`]).
//! - [`SignedRequestAuthProvider`], a [`cratestack_core::AuthProvider`]
//!   implementation wiring [`SignedRequestVerifier`] into a cratestack
//!   server.
//!
//! The `axum` Cargo feature (default-on) gates the items that touch the
//! `axum` crate itself: [`require_signed_request`], `jwks_router`, and the
//! `FromRequestParts` extractor impls on [`CurrentPrincipal`]/
//! [`AuthenticatedPrincipal`]. Everything else depends only on the plain
//! `http` crate for header/method/URI types, so a signing-only consumer
//! (e.g. a `cratestack-client`) can `default-features = false` to keep axum
//! (and its own tower/hyper/matchit dependency tree) out of its build.

mod authenticate;
mod context_mapping;
mod cose_enroll;
mod crypto_provider;
mod error;
mod id_token;
mod ids;
#[cfg(feature = "axum")]
mod middleware;
mod protocol;
mod provider;
#[cfg(feature = "axum")]
mod response;
mod service_signing;
mod signature_header;
mod signed_request;

pub const SIGNATURE_SCHEME: &str = "Signature";
pub const ID_TOKEN_GRANT: &str = "urn:cratestack:params:oauth:grant-type:id-sd-jwt";
pub const REFRESH_TOKEN_GRANT: &str = "refresh_token";
pub const ENROLL_CHALLENGE_COSE_KID: &str = "cratestack-auth-enroll-challenge-v1";
/// Env var carrying the URL-safe-base64-no-pad-encoded 32-byte Ed25519
/// seed used to sign/verify COSE enrolment challenge responses. See
/// `cose_enroll::challenge_signing_key`.
pub const CHALLENGE_SIGNING_KEY_ENV: &str = "CRATESTACK_AUTH_CHALLENGE_SIGNING_KEY";

pub use authenticate::{
    authenticate_cratestack_request, authenticate_cratestack_request_with, authorization_header,
    request_uri,
};
pub use context_mapping::principal_to_cratestack_context;
pub use cose_enroll::{build_cose_enroll_response, parse_cose_enroll_response};
pub use error::{AuthError, auth_error_to_cratestack_error};
pub use id_token::{
    AuthenticatedPrincipal, CurrentPrincipal, DEFAULT_ID_TOKEN_AUDIENCE, DisclosureClaim,
    ID_TOKEN_AUDIENCE_ENV, IdTokenClaims, IdTokenClaimsParams, IdTokenVerifier, IssuedSdIdToken,
    RequestPrincipal, UserPrincipal, decode_disclosures_unverified,
    decode_id_token_claims_unverified, decode_signing_key, default_id_token_claims,
    encode_signing_key, issue_id_token, issue_sd_id_token, issuer_jwk, take_disclosures,
    verifying_key_from_jwk, verifying_key_jwk,
};
pub use ids::{challenge, challenge_expiry, enrollment_id, key_id, user_id};
#[cfg(feature = "axum")]
pub use middleware::require_signed_request;
pub use protocol::{
    AuthorizationServerMetadata, Confirmation, DeviceSummary, EnrollRequest, EnrollResponse,
    IntrospectRequest, IntrospectResponse, Jwk, JwksDocument, KeySummary, NextStep, TokenRequest,
    TokenResponse, UserSummary, UserinfoResponse, VerifyRequest, VerifyResponse,
    authorization_server_metadata, jwks,
};
pub use provider::{SignedRequestAuthProvider, TransportCallerMode};
#[cfg(feature = "axum")]
pub use service_signing::jwks_router;
pub use service_signing::{
    MultiIssuerJwksVerifier, ServiceSigningKey, VerifiedToken, mint_signed_token,
};
pub use signature_header::{
    SignatureHeader, parse_bearer_token, parse_signature_header, uses_signature_scheme,
};
pub use signed_request::{
    DEFAULT_SIGNATURE_MAX_SKEW_SECONDS, DEFAULT_SIGNATURE_REPLAY_WINDOW_SECONDS, DeviceKeyResolver,
    NonceStore, SIGNATURE_MAX_SKEW_SECONDS_ENV, SIGNATURE_REPLAY_WINDOW_SECONDS_ENV,
    SIGNATURE_TRUSTED_ISSUERS_ENV, SIGNATURE_TRUSTED_KEYS_ENV, SignRequestParams,
    SignedRequestPrincipal, SignedRequestVerifier, canonical_query, canonical_signature_base,
    content_sha256_base64url, decode_signature_url_safe, decode_verifying_key,
    encode_verifying_key, nonce_store_from_redis_url, sign_request,
};

pub(crate) use crypto_provider::ensure_crypto_provider;
