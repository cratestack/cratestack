//! Single-use nonce enforcement: the [`NonceStore`] trait and the default
//! in-process implementation. See [`super::nonce_redis`] for the
//! Redis-backed store used across multiple server instances.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};

use crate::AuthError;

#[async_trait]
pub trait NonceStore: Send + Sync {
    async fn claim(
        &self,
        key_id: &str,
        nonce: &str,
        timestamp: DateTime<Utc>,
        replay_window: Duration,
    ) -> Result<(), AuthError>;
}

#[derive(Default)]
pub(super) struct InMemoryNonceStore {
    entries: Mutex<HashMap<String, DateTime<Utc>>>,
}

#[async_trait]
impl NonceStore for InMemoryNonceStore {
    async fn claim(
        &self,
        key_id: &str,
        nonce: &str,
        timestamp: DateTime<Utc>,
        replay_window: Duration,
    ) -> Result<(), AuthError> {
        let now = Utc::now();
        let expires_at = timestamp + replay_window;
        let storage_key = format!("{key_id}:{nonce}");
        let mut entries = self.entries.lock().map_err(|_| {
            AuthError::InvalidTrustedSigningKeys("nonce store poisoned".to_string())
        })?;

        entries.retain(|_, active_until| *active_until > now);
        if matches!(entries.get(&storage_key), Some(active_until) if *active_until > now) {
            return Err(AuthError::NonceReused);
        }

        entries.insert(storage_key, expires_at);
        Ok(())
    }
}
