use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Confirmation, Jwk};

pub const DEFAULT_ID_TOKEN_AUDIENCE: &str = "cratestack-issued-tokens";
pub const ID_TOKEN_AUDIENCE_ENV: &str = "CRATESTACK_AUTH_ID_TOKEN_AUDIENCE";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct IdTokenClaims {
    pub iss: String,
    pub sub: String,
    pub aud: String,
    pub azp: String,
    pub iat: i64,
    pub exp: i64,
    /// Unique token identifier (JWT `jti`). The issuer sets a fresh value on
    /// every mint so two tokens with otherwise-identical claims (same
    /// sub/iat/exp — possible when a short-lived id_jwt is refreshed within
    /// the same wall-clock second) are still byte-distinct JWTs. Optional +
    /// `#[serde(default)]` so synthetic/legacy tokens that omit it still
    /// decode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jti: Option<String>,
    pub cnf: Confirmation,
    #[serde(rename = "mainEmail", skip_serializing_if = "Option::is_none")]
    pub main_email: Option<String>,
    #[serde(rename = "mainPhone", skip_serializing_if = "Option::is_none")]
    pub main_phone: Option<String>,
    #[serde(rename = "mainAddress", skip_serializing_if = "Option::is_none")]
    pub main_address: Option<Value>,
    #[serde(rename = "profileVersion")]
    pub profile_version: i32,
    #[serde(rename = "enrollmentStatus")]
    pub enrollment_status: String,
    #[serde(rename = "kycStatus", skip_serializing_if = "Option::is_none")]
    pub kyc_status: Option<String>,
    /// Server-issued authorization role. Carried as a *verified* claim that
    /// the issuer derives from its own user store — never from a
    /// caller-supplied `client_id`/`azp`. Verifiers gate elevated access on
    /// this claim. Defaults to `"user"` so tokens minted before this claim
    /// existed (and any third-party token without it) are non-privileged.
    #[serde(default = "default_role")]
    pub role: String,
    /// SD-JWT selectively-disclosable claim digests (base64url(SHA-256(disclosure))).
    #[serde(default, rename = "_sd", skip_serializing_if = "Vec::is_empty")]
    pub sd: Vec<String>,
    /// SD-JWT digest hash algorithm — fixed to `sha-256`.
    #[serde(default, rename = "_sd_alg", skip_serializing_if = "Option::is_none")]
    pub sd_alg: Option<String>,
}

/// The non-privileged default role. Used by serde as the `role` claim default
/// (absent claim → `"user"`) and by [`super::issuance::default_id_token_claims`].
pub(super) fn default_role() -> String {
    "user".to_owned()
}

pub struct IdTokenClaimsParams<'a> {
    pub issuer: &'a str,
    pub client_id: &'a str,
    pub subject: &'a str,
    pub bound_key_id: &'a str,
    /// The holder's bound public key as an OKP/Ed25519 JWK, embedded in `cnf.jwk`
    /// so JWKS-verifying services can check device-signed requests without a
    /// device-key registry. Pass `None` for service-to-service tokens whose
    /// `kid` is resolvable via static trust / JWKS.
    pub bound_key_jwk: Option<Jwk>,
    pub profile_version: i32,
    pub enrollment_status: &'a str,
    pub kyc_status: Option<String>,
    pub main_email: Option<String>,
    pub main_phone: Option<String>,
    pub main_address: Option<Value>,
    /// SD-JWT selectively-disclosable claims to attach as `_sd` digests in the issued
    /// token. Each disclosure carries its own salt so distinct holders don't share
    /// digests for the same claim/value pair.
    pub disclosures: Vec<DisclosureClaim>,
}

/// A claim that the issuer adds to the SD-JWT as a digest in `_sd[]` and ships out
/// alongside the JWT in the compact form `<jwt>~<disclosure_b64>~`.
#[derive(Clone, Debug)]
pub struct DisclosureClaim {
    pub name: String,
    pub value: Value,
}

/// Serialized output of [`super::issuance::issue_sd_id_token`]: the SD-JWT compact
/// form (`<jwt>~d1~d2~`) plus the parsed disclosure strings (so the caller can
/// persist them with the issued token if it wants to reissue without re-running
/// the cuid generator).
#[derive(Clone, Debug)]
pub struct IssuedSdIdToken {
    pub compact: String,
    pub jwt: String,
    pub disclosures: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct JwtHeader {
    pub(super) alg: String,
    pub(super) typ: String,
    pub(super) kid: String,
}
