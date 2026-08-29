//! The token exchange (`/token`) plus the `/userinfo` and `/introspect`
//! responses that read off an issued token.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::metadata::Jwk;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TokenRequest {
    #[serde(rename = "grant_type")]
    pub grant_type: String,
    #[serde(rename = "client_id")]
    pub client_id: String,
    #[serde(rename = "device_key_id")]
    pub device_key_id: Option<String>,
    #[serde(rename = "subject_token")]
    pub subject_token: Option<String>,
    #[serde(rename = "refresh_token")]
    pub refresh_token: Option<String>,
    pub scope: Option<String>,
    #[serde(rename = "profile_version_hint")]
    pub profile_version_hint: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TokenResponse {
    #[serde(rename = "token_type")]
    pub token_type: String,
    #[serde(rename = "issued_token_type")]
    pub issued_token_type: String,
    #[serde(rename = "id_jwt")]
    pub id_jwt: String,
    #[serde(rename = "expires_in")]
    pub expires_in: i64,
    #[serde(rename = "refresh_token")]
    pub refresh_token: String,
    pub cnf: Confirmation,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Confirmation {
    pub kid: String,
    /// The holder's bound public key, as an OKP/Ed25519 JWK. When present, a
    /// JWKS-verified id_jwt lets any service verify a request signed by this
    /// device key WITHOUT its own device-key registry — the issuer vouches for
    /// the key (proof-of-possession). Absent on service-to-service tokens that
    /// bind a `kid` resolvable via static trust / JWKS.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jwk: Option<Jwk>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct UserinfoResponse {
    pub sub: String,
    #[serde(rename = "mainEmail")]
    pub main_email: Option<String>,
    #[serde(rename = "mainPhone")]
    pub main_phone: Option<String>,
    #[serde(rename = "mainAddress")]
    pub main_address: Option<Value>,
    #[serde(rename = "profileVersion")]
    pub profile_version: i32,
    #[serde(rename = "enrollmentStatus")]
    pub enrollment_status: String,
    #[serde(rename = "kycStatus")]
    pub kyc_status: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct IntrospectRequest {
    pub token: String,
    #[serde(rename = "token_type_hint")]
    pub token_type_hint: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct IntrospectResponse {
    pub active: bool,
    pub sub: Option<String>,
    pub iss: Option<String>,
    pub exp: Option<i64>,
    pub kid: Option<String>,
}
