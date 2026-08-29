#![cfg(test)]

use std::collections::HashMap;

use crate::AuthError;

use super::tests_fixtures::{UploadTicketClaims, fixture_signing_key, future_exp, past_exp};
use super::{MultiIssuerJwksVerifier, ServiceSigningKey};

#[tokio::test]
async fn verifier_rejects_token_from_untrusted_issuer() {
    let key = ServiceSigningKey::new(
        "vendor-service",
        "vendor-service-test",
        fixture_signing_key(),
    );
    let claims = UploadTicketClaims {
        iss: "vendor-service".into(),
        sub: "user_123".into(),
        iat: chrono::Utc::now().timestamp(),
        exp: future_exp(),
        owner_type: "vendor".into(),
        owner_id: "vnd_1".into(),
        purpose: "vendor_logo".into(),
        nonce: "n1".into(),
    };
    let token = key.mint(&claims).unwrap();

    // Trust list does NOT include vendor-service.
    let verifier = MultiIssuerJwksVerifier::new(HashMap::from([(
        "catalog-service".to_string(),
        "http://127.0.0.1:0/jwks.json".to_string(),
    )]))
    .unwrap();

    let err = verifier
        .verify::<UploadTicketClaims>(&token)
        .await
        .expect_err("must reject untrusted issuer");
    assert!(matches!(err, AuthError::UntrustedIssuer(ref iss) if iss == "vendor-service"));
}

#[tokio::test]
async fn verifier_rejects_expired_token() {
    let key = ServiceSigningKey::new(
        "vendor-service",
        "vendor-service-test",
        fixture_signing_key(),
    );
    let claims = UploadTicketClaims {
        iss: "vendor-service".into(),
        sub: "user_123".into(),
        iat: chrono::Utc::now().timestamp() - 600,
        exp: past_exp(),
        owner_type: "vendor".into(),
        owner_id: "vnd_1".into(),
        purpose: "vendor_logo".into(),
        nonce: "n1".into(),
    };
    let token = key.mint(&claims).unwrap();

    // Even the trusted path rejects expired.
    let verifier = MultiIssuerJwksVerifier::new(HashMap::from([(
        "vendor-service".to_string(),
        "http://127.0.0.1:0/jwks.json".to_string(),
    )]))
    .unwrap();

    let err = verifier
        .verify::<UploadTicketClaims>(&token)
        .await
        .expect_err("must reject expired token");
    assert!(matches!(err, AuthError::IdTokenExpired));
}

#[test]
fn trusted_issuers_are_alphabetised() {
    let verifier = MultiIssuerJwksVerifier::new(HashMap::from([
        ("vendor-service".to_string(), "x".to_string()),
        ("catalog-service".to_string(), "y".to_string()),
        ("order-service".to_string(), "z".to_string()),
    ]))
    .unwrap();
    assert_eq!(
        verifier.trusted_issuers(),
        vec!["catalog-service", "order-service", "vendor-service"],
    );
}
