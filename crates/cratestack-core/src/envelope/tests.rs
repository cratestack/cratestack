use super::*;

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
