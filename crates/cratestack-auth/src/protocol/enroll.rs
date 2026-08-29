//! The device enrolment exchange: `/enroll` request/response and the
//! `/verify` challenge-response that completes it.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EnrollRequest {
    #[serde(rename = "deviceName")]
    pub device_name: String,
    pub platform: String,
    #[serde(rename = "publicKey")]
    pub public_key: Option<String>,
    #[serde(rename = "publicJwk")]
    pub public_jwk: Option<Value>,
    #[serde(rename = "proposedKeyId")]
    pub proposed_key_id: Option<String>,
    #[serde(rename = "appVersion")]
    pub app_version: Option<String>,
    #[serde(rename = "clientInfo")]
    pub client_info: Option<Value>,
    #[serde(rename = "antiAbuse")]
    pub anti_abuse: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EnrollResponse {
    #[serde(rename = "enrollmentId")]
    pub enrollment_id: String,
    #[serde(rename = "keyId")]
    pub key_id: String,
    pub challenge: String,
    #[serde(rename = "challengeFormat")]
    pub challenge_format: String,
    #[serde(rename = "expiresAt")]
    pub expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct VerifyRequest {
    #[serde(rename = "enrollmentId")]
    pub enrollment_id: String,
    #[serde(rename = "challengeResponse")]
    pub challenge_response: String,
    #[serde(rename = "deviceProof")]
    pub device_proof: Option<String>,
    #[serde(rename = "clientInfo")]
    pub client_info: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct VerifyResponse {
    pub user: UserSummary,
    pub device: DeviceSummary,
    pub key: KeySummary,
    #[serde(rename = "nextStep")]
    pub next_step: NextStep,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct UserSummary {
    pub id: String,
    #[serde(rename = "enrollmentStatus")]
    pub enrollment_status: String,
    #[serde(rename = "kycStatus")]
    pub kyc_status: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DeviceSummary {
    #[serde(rename = "trustStatus")]
    pub trust_status: String,
    #[serde(rename = "deviceName")]
    pub device_name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KeySummary {
    #[serde(rename = "keyId")]
    pub key_id: String,
    pub status: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NextStep {
    #[serde(rename = "canRequestIdJwt")]
    pub can_request_id_jwt: bool,
}
