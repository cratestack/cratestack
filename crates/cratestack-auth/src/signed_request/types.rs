//! Plain data types carried across the sign/verify boundary.

use chrono::{DateTime, Utc};
use ed25519_dalek::SigningKey;
use http::Method;

pub struct SignRequestParams<'a> {
    pub signing_key: &'a SigningKey,
    pub method: &'a Method,
    pub path: &'a str,
    pub query: Option<&'a str>,
    pub body: &'a [u8],
    pub timestamp: &'a str,
    pub nonce: &'a str,
    pub key_id: &'a str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignedRequestPrincipal {
    pub key_id: String,
    pub timestamp: DateTime<Utc>,
    pub nonce: String,
    pub id_jwt: Option<String>,
    pub alg: Option<String>,
    pub content_sha256: String,
    /// True when the signing key was resolved ONLY via the cnf-bound id_jwt
    /// proof-of-possession fallback — i.e. this is an end-user device, not a
    /// statically-trusted service key or a JWKS/registry-resolved key. Internal
    /// service-to-service middleware (`require_signed_request`) rejects these so
    /// an enrolled user can't reach `/internal/*` endpoints meant for services.
    pub via_id_token_pop: bool,
}
