use std::sync::Arc;

use super::*;

#[tokio::test]
async fn hmac_envelope_round_trip_succeeds() {
    let keys = Arc::new(StaticKeyProvider::new().with_key("ops-1", vec![0xaa; 32]));
    let env = HmacEnvelope::new(keys.clone(), "ops-1");
    let payload = serde_json::json!({ "transfer": { "amount": "100.00" } });
    let sealed = env.seal(payload.clone()).await.expect("seal");
    let opened = env.open(&sealed).await.expect("open");
    assert_eq!(opened, payload);
}

#[tokio::test]
async fn hmac_envelope_rejects_modified_body() {
    let keys = Arc::new(StaticKeyProvider::new().with_key("ops-1", vec![0xaa; 32]));
    let env = HmacEnvelope::new(keys.clone(), "ops-1");
    let mut sealed = env
        .seal(serde_json::json!({ "amount": "100" }))
        .await
        .expect("seal");
    sealed.body = serde_json::json!({ "amount": "999" });
    let err = env.open(&sealed).await.expect_err("must reject tamper");
    assert_eq!(err.code(), "UNAUTHORIZED");
}

#[tokio::test]
async fn hmac_envelope_rejects_stale_timestamp() {
    let keys = Arc::new(StaticKeyProvider::new().with_key("ops-1", vec![0xaa; 32]));
    let env = HmacEnvelope::new(keys.clone(), "ops-1").with_clock_skew_secs(1);
    let mut sealed = env.seal(serde_json::json!({})).await.expect("seal");
    // Push the timestamp into the past beyond the skew window.
    sealed.ts -= 60;
    // Recompute MAC to ensure the envelope is structurally valid —
    // we want to isolate that the timestamp window is what blocks it.
    use base64::Engine;
    use hmac::{Hmac, Mac};
    let key = keys.resolve_signing_key("ops-1").await.expect("key");
    let mut mac = <Hmac<sha2::Sha256> as Mac>::new_from_slice(&key).unwrap();
    mac.update(&sealed.signing_input().unwrap());
    sealed.mac_b64 =
        base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());

    let err = env.open(&sealed).await.expect_err("must reject");
    assert_eq!(err.code(), "UNAUTHORIZED");
}

#[tokio::test]
async fn hmac_envelope_with_nonce_store_rejects_replays() {
    let keys = Arc::new(StaticKeyProvider::new().with_key("ops-1", vec![0xaa; 32]));
    let nonces: Arc<dyn NonceStore> = Arc::new(InMemoryNonceStore::new());
    let env = HmacEnvelope::new(keys.clone(), "ops-1").with_nonce_store(nonces.clone());
    let sealed = env
        .seal(serde_json::json!({ "amount": "1" }))
        .await
        .expect("seal");
    env.open(&sealed).await.expect("first open succeeds");
    let err = env.open(&sealed).await.expect_err("replay must fail");
    assert_eq!(err.code(), "UNAUTHORIZED");
}

#[tokio::test]
async fn nonce_store_purges_expired_entries() {
    let store = InMemoryNonceStore::new();
    let past = chrono::Utc::now() - chrono::Duration::seconds(60);
    // Insert an already-expired entry, then attempt to record the same
    // nonce again — the GC inside `record_if_unseen` should evict it.
    assert!(store.record_if_unseen("n1", past).await.unwrap());
    assert!(
        store
            .record_if_unseen("n1", past + chrono::Duration::seconds(120))
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn hmac_envelope_rejects_unknown_alg() {
    let keys = Arc::new(StaticKeyProvider::new().with_key("ops-1", vec![0xaa; 32]));
    let env = HmacEnvelope::new(keys.clone(), "ops-1");
    let mut sealed = env.seal(serde_json::json!({})).await.expect("seal");
    sealed.alg = "none".to_owned();
    let err = env.open(&sealed).await.expect_err("must reject");
    assert_eq!(err.code(), "UNAUTHORIZED");
}
