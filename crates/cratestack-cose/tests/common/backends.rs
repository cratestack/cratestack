//! Fake key resolvers and nonce stores.

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use cratestack_core::{CratestackError, InMemoryNonceStore, NonceStore};
use cratestack_cose::{CoseAlg, CoseVerifierResolver, CoseVerifyKey};

/// Returns the same candidates for every `(kid, alg)`, ignoring both, the
/// way a careless resolver might. Used to show that the opener, not the
/// resolver, is what keeps key types apart.
pub struct FixedResolver(pub Vec<CoseVerifyKey>);

#[async_trait::async_trait]
impl CoseVerifierResolver for FixedResolver {
    async fn resolve(
        &self,
        _kid: &[u8],
        _alg: CoseAlg,
    ) -> Result<Vec<CoseVerifyKey>, CratestackError> {
        Ok(self.0.clone())
    }
}

/// A resolver whose backend is down.
pub struct FailingResolver;

#[async_trait::async_trait]
impl CoseVerifierResolver for FailingResolver {
    async fn resolve(
        &self,
        _kid: &[u8],
        _alg: CoseAlg,
    ) -> Result<Vec<CoseVerifyKey>, CratestackError> {
        Err(CratestackError::Unavailable("key database down".to_owned()))
    }
}

/// A nonce store whose backend is down.
pub struct FailingNonceStore;

#[async_trait::async_trait]
impl NonceStore for FailingNonceStore {
    async fn record_if_unseen(
        &self,
        _nonce: &str,
        _expires_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, CratestackError> {
        Err(CratestackError::Unavailable("redis down".to_owned()))
    }
}

/// An in-memory store that also records every call it receives.
#[derive(Default)]
pub struct RecordingNonceStore {
    inner: InMemoryNonceStore,
    pub calls: AtomicUsize,
    pub keys: Mutex<Vec<(String, chrono::DateTime<chrono::Utc>)>>,
}

impl RecordingNonceStore {
    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait::async_trait]
impl NonceStore for RecordingNonceStore {
    async fn record_if_unseen(
        &self,
        nonce: &str,
        expires_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, CratestackError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.keys
            .lock()
            .expect("lock")
            .push((nonce.to_owned(), expires_at));
        self.inner.record_if_unseen(nonce, expires_at).await
    }
}
