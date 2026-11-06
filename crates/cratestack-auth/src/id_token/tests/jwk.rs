use ed25519_dalek::SigningKey;

use super::{TEST_ISSUER_SIGNING_KID, test_issuer_jwk};
use crate::{
    AuthError,
    id_token::{verifying_key_from_jwk, verifying_key_jwk},
};

#[test]
fn publishes_full_dev_issuer_jwk() {
    let jwk = test_issuer_jwk();
    assert_eq!(jwk.kid, TEST_ISSUER_SIGNING_KID);
    assert_eq!(jwk.kty, "OKP");
    assert_eq!(jwk.crv.as_deref(), Some("Ed25519"));
    assert!(jwk.x.is_some());
}

#[test]
fn verifying_key_jwk_roundtrips() {
    let key = SigningKey::from_bytes(&[9u8; 32]);
    let jwk = verifying_key_jwk(&key.verifying_key(), "vk_device");
    assert_eq!(jwk.kty, "OKP");
    assert_eq!(jwk.crv.as_deref(), Some("Ed25519"));
    assert_eq!(jwk.kid, "vk_device");
    let recovered = verifying_key_from_jwk(&jwk).expect("jwk should decode");
    assert_eq!(recovered, key.verifying_key());

    // Wrong curve / missing x are rejected.
    let mut bad_crv = jwk.clone();
    bad_crv.crv = Some("P-256".to_string());
    assert!(matches!(
        verifying_key_from_jwk(&bad_crv),
        Err(AuthError::InvalidPublicKey(_))
    ));
    let mut no_x = jwk.clone();
    no_x.x = None;
    assert!(matches!(
        verifying_key_from_jwk(&no_x),
        Err(AuthError::InvalidPublicKey(_))
    ));
}

/// RFC 8037 Appendix A.3: the thumbprint of the Ed25519 key of A.2.
#[test]
fn jwk_thumbprint_matches_rfc_8037() {
    let mut jwk = crate::id_token::verifying_key_jwk(
        &SigningKey::from_bytes(&[9u8; 32]).verifying_key(),
        "ignored",
    );
    jwk.x = Some("11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo".to_owned());
    assert_eq!(
        crate::jwk_thumbprint(&jwk).as_deref(),
        Some("kPrK_qmxVWaYVA9wwBF6Iuo3vVzz7TxHCTwXBygrS4k")
    );
    jwk.x = None;
    assert_eq!(crate::jwk_thumbprint(&jwk), None);
    jwk.kty = "EC".to_owned();
    assert_eq!(crate::jwk_thumbprint(&jwk), None);
}
