#![cfg(test)]

use crate::AuthError;

use super::ServiceSigningKey;
use super::tests_fixtures::{UploadTicketClaims, future_exp};

#[test]
fn ephemeral_signing_key_round_trips_through_mint() {
    let key = ServiceSigningKey::ephemeral("vendor-service", "vendor-service-test");
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
    let token = key.mint(&claims).expect("mint");
    // Compact form: three segments separated by '.'.
    assert_eq!(token.matches('.').count(), 2);
}

#[test]
fn from_env_returns_missing_signing_key_env_when_unset() {
    // Use a name that's exceedingly unlikely to be set in CI.
    // We pattern-match instead of `expect_err` so ServiceSigningKey
    // doesn't have to derive `Debug` (which would leak key bytes).
    match ServiceSigningKey::from_env(
        "vendor-service",
        "vendor-service-test",
        "CRATESTACK_AUTH_TEST_DEFINITELY_NOT_SET_KEY_a8c8e",
    ) {
        Ok(_) => panic!("from_env must error when its env var is unset"),
        Err(err) => {
            assert!(matches!(err, AuthError::MissingSigningKeyEnv(_)));
        }
    }
}
