//! Nonce-store plumbing: storage-key/TTL helpers, a custom `NonceStore`
//! plugged into the verifier, and Redis nonce-store configuration/failure
//! handling.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{Duration, SecondsFormat, Utc};
use http::Method;

use super::{example_key_id, example_signing_key};
use crate::AuthError;
use crate::signed_request::nonce_redis::{nonce_storage_key, replay_ttl_seconds};
use crate::signed_request::{NonceStore, SignRequestParams, SignedRequestVerifier, sign_request};

#[test]
fn builds_nonce_storage_keys() {
    assert_eq!(
        nonce_storage_key("vk_123", "n_456"),
        "cratestack:signature-nonce:vk_123:n_456"
    );
}

#[test]
fn replay_ttl_uses_remaining_window() {
    let ttl = replay_ttl_seconds(Utc::now() - Duration::seconds(60), Duration::seconds(300));
    assert!((239..=240).contains(&ttl));
}

#[tokio::test]
async fn supports_custom_nonce_store() {
    let signing_key = example_signing_key();
    let verifier = SignedRequestVerifier::new([(example_key_id(), signing_key.verifying_key())])
        .with_nonce_store(Arc::new(RejectingNonceStore));
    let timestamp = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let signature = sign_request(SignRequestParams {
        signing_key: &signing_key,
        method: &Method::GET,
        path: "/vendors",
        query: None,
        body: b"",
        timestamp: &timestamp,
        nonce: "nonce-3",
        key_id: example_key_id().as_str(),
    });
    let header = format!(
        "Signature keyId=\"{}\", timestamp=\"{}\", nonce=\"nonce-3\", signature=\"{}\"",
        example_key_id(),
        timestamp,
        signature,
    );

    let result = verifier
        .verify(&Method::GET, &"/vendors".parse().unwrap(), b"", &header)
        .await;
    assert!(matches!(result, Err(AuthError::NonceReused)));
}

#[test]
fn rejects_invalid_redis_nonce_store_configuration() {
    let verifier =
        SignedRequestVerifier::new([(example_key_id(), example_signing_key().verifying_key())]);
    let error = verifier
        .with_redis_nonce_store("not-a-redis-url")
        .err()
        .expect("invalid redis urls should be rejected");

    assert!(matches!(
        error,
        AuthError::InvalidNonceStoreConfiguration(_)
    ));
}

#[tokio::test]
async fn fails_closed_when_redis_nonce_store_is_unavailable() {
    let signing_key = example_signing_key();
    let verifier = SignedRequestVerifier::new([(example_key_id(), signing_key.verifying_key())])
        .with_redis_nonce_store("redis://127.0.0.1:1/")
        .expect("redis url should parse");
    let timestamp = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let signature = sign_request(SignRequestParams {
        signing_key: &signing_key,
        method: &Method::GET,
        path: "/vendors",
        query: None,
        body: b"",
        timestamp: &timestamp,
        nonce: "nonce-redis-down",
        key_id: example_key_id().as_str(),
    });
    let header = format!(
        "Signature keyId=\"{}\", timestamp=\"{}\", nonce=\"nonce-redis-down\", signature=\"{}\"",
        example_key_id(),
        timestamp,
        signature,
    );

    let result = verifier
        .verify(&Method::GET, &"/vendors".parse().unwrap(), b"", &header)
        .await;
    assert!(matches!(result, Err(AuthError::NonceStoreUnavailable(_))));
}

struct RejectingNonceStore;

#[async_trait]
impl NonceStore for RejectingNonceStore {
    async fn claim(
        &self,
        _key_id: &str,
        _nonce: &str,
        _timestamp: chrono::DateTime<Utc>,
        _replay_window: chrono::Duration,
    ) -> Result<(), AuthError> {
        Err(AuthError::NonceReused)
    }
}
