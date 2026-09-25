//! The in-process signers against published vectors, and the algorithm
//! allowlist.

mod common;

use common::{unhex, unhex32};
use cratestack_cose::{
    CoseAlg, CoseMode, CoseSigner, CoseVerifyKey, Ed25519Signer, HmacSigner, MIN_HMAC_SECRET_LEN,
    P256Signer,
};

#[tokio::test]
async fn ed25519_signer_matches_rfc8032_test_1() {
    let signer = Ed25519Signer::from_seed(&unhex32(
        "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60",
    ));
    let signature = signer.sign(b"").await.expect("sign");
    assert_eq!(
        signature,
        unhex(
            "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b"
        )
    );
    assert_eq!(
        signer.verify_key(),
        CoseVerifyKey::ed25519(&unhex32(
            "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a"
        ))
        .expect("public key")
    );
}

#[tokio::test]
async fn p256_signer_is_rfc6979_deterministic() {
    // RFC 6979 A.2.5, P-256 with SHA-256, message "sample".
    let signer = P256Signer::from_scalar(&unhex32(common::P256_SCALAR)).expect("scalar");
    let signature = signer.sign(b"sample").await.expect("sign");
    assert_eq!(
        signature,
        unhex(
            "efd48b2aacb6a8fd1140dd9cd45e81d69d2c877b56aaf991c34d0ea84eaf3716f7cb1c942d657c41d436c7a1b6e29f65f3e900dbb9aff4064dc4ab2f843acda8"
        )
    );
    assert_eq!(signer.alg(), CoseAlg::Esp256);
}

#[tokio::test]
async fn hmac_signer_matches_rfc4231_test_6_and_truncates_for_256_64() {
    // RFC 4231 test case 6: a 131-byte key of 0xaa.
    let key = vec![0xaa; 131];
    let data = b"Test Using Larger Than Block-Size Key - Hash Key First";
    let full = "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54";
    let signer = HmacSigner::new(CoseAlg::Hmac256_256, key.clone()).expect("key");
    assert_eq!(signer.sign(data).await.expect("mac"), unhex(full));
    let truncated = HmacSigner::new(CoseAlg::Hmac256_64, key).expect("key");
    assert_eq!(truncated.sign(data).await.expect("mac"), unhex(&full[..16]));
}

#[test]
fn hmac_secrets_below_32_bytes_are_refused() {
    let short = vec![7; MIN_HMAC_SECRET_LEN - 1];
    assert!(HmacSigner::new(CoseAlg::Hmac256_256, short.clone()).is_err());
    assert!(CoseVerifyKey::hmac(CoseAlg::Hmac256_256, short).is_err());
    assert!(CoseVerifyKey::hmac(CoseAlg::Ed25519, vec![7; 32]).is_err());
    assert!(HmacSigner::new(CoseAlg::Hmac256_256, vec![7; MIN_HMAC_SECRET_LEN]).is_ok());
}

#[test]
fn hmac_signer_refuses_a_signature_algorithm() {
    assert!(HmacSigner::new(CoseAlg::Ed25519, vec![7; 32]).is_err());
}

#[test]
fn p256_signer_refuses_invalid_scalars() {
    assert!(P256Signer::from_scalar(&[0; 32]).is_err());
    assert!(P256Signer::from_scalar(&[0xff; 32]).is_err());
}

#[test]
fn only_four_algorithm_ids_exist() {
    for &alg in CoseAlg::ALL {
        assert_eq!(CoseAlg::from_id(alg.id()), Some(alg));
    }
    // -8 (EdDSA) and -7 (ES256) are the deprecated polymorphic ids that
    // RFC 9864 replaces with -19 and -9; 6/7 are HMAC 384/512.
    for id in [-8, -7, -35, -36, -37, -257, 0, 1, 6, 7, -18, -20, -10] {
        assert_eq!(CoseAlg::from_id(id), None, "{id} must be refused");
    }
}

#[test]
fn algorithms_know_their_mode_and_signature_length() {
    assert_eq!(CoseAlg::Ed25519.mode(), CoseMode::Sign1);
    assert_eq!(CoseAlg::Esp256.mode(), CoseMode::Sign1);
    assert_eq!(CoseAlg::Hmac256_64.mode(), CoseMode::Mac0);
    assert_eq!(CoseAlg::Hmac256_256.mode(), CoseMode::Mac0);
    assert_eq!(CoseAlg::Hmac256_64.signature_len(), 8);
    assert_eq!(CoseAlg::Hmac256_256.signature_len(), 32);
    assert_eq!(CoseMode::Sign1.tag(), 18);
    assert_eq!(CoseMode::Mac0.tag(), 17);
}

#[test]
fn debug_output_never_shows_a_secret() {
    let signer = HmacSigner::new(CoseAlg::Hmac256_256, vec![0x5a; 32]).expect("key");
    let builder = cratestack_cose::CoseEnvelope::client(
        CoseMode::Mac0,
        std::sync::Arc::new(signer.clone()),
        std::sync::Arc::new(
            cratestack_cose::StaticVerifierResolver::new().with_key(signer.verify_key()),
        ),
    );
    let builder_rendered = format!("{builder:?}");
    assert!(builder_rendered.contains("Mac0") && builder_rendered.contains("skew_secs"));
    let envelope = builder.build().expect("build");
    let rendered = format!(
        "{signer:?} {:?} {builder_rendered} {envelope:?}",
        signer.verify_key()
    );
    assert!(!rendered.contains("5a, 5a"), "{rendered}");
    assert!(!rendered.to_lowercase().contains("5a5a"), "{rendered}");
}
