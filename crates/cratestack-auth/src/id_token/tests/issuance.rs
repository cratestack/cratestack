use serde_json::json;

use super::issue_token_pair;
use crate::id_token::{
    IdTokenClaimsParams, decode_id_token_claims_unverified, default_id_token_claims,
};

#[test]
fn issues_signed_id_jwt_tokens() {
    let claims = default_id_token_claims(IdTokenClaimsParams {
        issuer: "https://issuer.example",
        client_id: "example-client",
        subject: "usr_123",
        bound_key_id: "vk_123",
        bound_key_jwk: None,
        profile_version: 7,
        enrollment_status: "enrolled",
        kyc_status: Some("approved".to_string()),
        main_email: Some("user@example.com".to_string()),
        main_phone: None,
        main_address: Some(json!({ "country": "CM" })),
        disclosures: Vec::new(),
    });

    let issued = issue_token_pair(claims.clone()).expect("token pair should issue");
    assert_eq!(issued.cnf.kid, "vk_123");
    assert_eq!(issued.id_jwt.split('.').count(), 3);

    let decoded = decode_id_token_claims_unverified(&issued.id_jwt)
        .expect("jwt claims should decode without verification");
    assert_eq!(decoded, claims);
}
