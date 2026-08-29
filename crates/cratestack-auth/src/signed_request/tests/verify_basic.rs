//! Core `verify()` happy path plus timestamp-handling edge cases (stale
//! skew, non-UTC offsets, explicit `+00:00`) and nonce-replay rejection.

use chrono::{Duration, SecondsFormat, Utc};
use http::Method;

use super::{example_key_id, example_signing_key};
use crate::AuthError;
use crate::signed_request::{
    DEFAULT_SIGNATURE_MAX_SKEW_SECONDS, SignRequestParams, SignedRequestVerifier,
    content_sha256_base64url, sign_request,
};

#[tokio::test]
async fn verifies_signed_requests_and_rejects_reused_nonces() {
    let signing_key = example_signing_key();
    let verifier = SignedRequestVerifier::new([(example_key_id(), signing_key.verifying_key())]);
    let timestamp = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let signature = sign_request(SignRequestParams {
        signing_key: &signing_key,
        method: &Method::POST,
        path: "/uploads/presign",
        query: None,
        body: br#"{"purpose":"vendorLogo"}"#,
        timestamp: &timestamp,
        nonce: "nonce-1",
        key_id: example_key_id().as_str(),
    });
    let header = format!(
        "Signature keyId=\"{}\", timestamp=\"{}\", nonce=\"nonce-1\", signature=\"{}\", alg=\"Ed25519\", content_sha256=\"{}\"",
        example_key_id(),
        timestamp,
        signature,
        content_sha256_base64url(br#"{"purpose":"vendorLogo"}"#),
    );

    let principal = verifier
        .verify(
            &Method::POST,
            &"/uploads/presign".parse().unwrap(),
            br#"{"purpose":"vendorLogo"}"#,
            &header,
        )
        .await
        .expect("signature should verify");
    assert_eq!(principal.key_id, example_key_id());
    // A statically-trusted key is NOT a PoP fallback caller.
    assert!(!principal.via_id_token_pop);

    let reused = verifier
        .verify(
            &Method::POST,
            &"/uploads/presign".parse().unwrap(),
            br#"{"purpose":"vendorLogo"}"#,
            &header,
        )
        .await;
    assert!(matches!(reused, Err(AuthError::NonceReused)));
}

#[tokio::test]
async fn rejects_stale_timestamps() {
    let signing_key = example_signing_key();
    let verifier = SignedRequestVerifier::new([(example_key_id(), signing_key.verifying_key())]);
    let timestamp = (Utc::now() - Duration::seconds(DEFAULT_SIGNATURE_MAX_SKEW_SECONDS + 30))
        .to_rfc3339_opts(SecondsFormat::Secs, true);
    let signature = sign_request(SignRequestParams {
        signing_key: &signing_key,
        method: &Method::GET,
        path: "/vendors",
        query: Some("limit=20"),
        body: b"",
        timestamp: &timestamp,
        nonce: "nonce-2",
        key_id: example_key_id().as_str(),
    });
    let header = format!(
        "Signature keyId=\"{}\", timestamp=\"{}\", nonce=\"nonce-2\", signature=\"{}\"",
        example_key_id(),
        timestamp,
        signature,
    );

    let result = verifier
        .verify(
            &Method::GET,
            &"/vendors?limit=20".parse().unwrap(),
            b"",
            &header,
        )
        .await;

    assert!(matches!(
        result,
        Err(AuthError::SignatureTimestampOutOfWindow)
    ));
}

#[tokio::test]
async fn rejects_non_utc_timestamp_offsets() {
    let signing_key = example_signing_key();
    let verifier = SignedRequestVerifier::new([(example_key_id(), signing_key.verifying_key())]);
    let timestamp = "2026-04-24T12:00:00+01:00";
    let signature = sign_request(SignRequestParams {
        signing_key: &signing_key,
        method: &Method::GET,
        path: "/vendors",
        query: None,
        body: b"",
        timestamp,
        nonce: "nonce-utc",
        key_id: example_key_id().as_str(),
    });
    let header = format!(
        "Signature keyId=\"{}\", timestamp=\"{}\", nonce=\"nonce-utc\", signature=\"{}\"",
        example_key_id(),
        timestamp,
        signature,
    );

    let result = verifier
        .verify(&Method::GET, &"/vendors".parse().unwrap(), b"", &header)
        .await;
    assert!(matches!(
        result,
        Err(AuthError::InvalidSignatureTimestamp(_))
    ));
}

#[tokio::test]
async fn accepts_explicit_utc_offset() {
    let signing_key = example_signing_key();
    let verifier = SignedRequestVerifier::new([(example_key_id(), signing_key.verifying_key())]);
    let timestamp = Utc::now()
        .to_rfc3339_opts(SecondsFormat::Secs, false)
        .replace('Z', "+00:00");
    let signature = sign_request(SignRequestParams {
        signing_key: &signing_key,
        method: &Method::GET,
        path: "/vendors",
        query: None,
        body: b"",
        timestamp: &timestamp,
        nonce: "nonce-utc-zero",
        key_id: example_key_id().as_str(),
    });
    let header = format!(
        "Signature keyId=\"{}\", timestamp=\"{}\", nonce=\"nonce-utc-zero\", signature=\"{}\"",
        example_key_id(),
        timestamp,
        signature,
    );

    let principal = verifier
        .verify(&Method::GET, &"/vendors".parse().unwrap(), b"", &header)
        .await
        .expect("+00:00 timestamps should verify");
    assert_eq!(principal.key_id, example_key_id());
}
