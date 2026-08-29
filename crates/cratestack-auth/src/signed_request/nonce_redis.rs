//! Redis-backed [`NonceStore`], and the public factory that picks between
//! it and the in-memory default based on configuration.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use redis::Client as RedisClient;

use super::consts::REDIS_NONCE_KEY_PREFIX;
use super::nonce_store::{InMemoryNonceStore, NonceStore};
use crate::AuthError;

pub(super) struct RedisNonceStore {
    client: RedisClient,
}

impl RedisNonceStore {
    pub(super) fn new(redis_url: &str) -> Result<Self, AuthError> {
        let client = RedisClient::open(redis_url).map_err(|error| {
            AuthError::InvalidNonceStoreConfiguration(format!(
                "invalid redis url for nonce store: {error}"
            ))
        })?;
        Ok(Self { client })
    }
}

#[async_trait]
impl NonceStore for RedisNonceStore {
    async fn claim(
        &self,
        key_id: &str,
        nonce: &str,
        timestamp: DateTime<Utc>,
        replay_window: Duration,
    ) -> Result<(), AuthError> {
        let mut connection = self
            .client
            .get_multiplexed_async_connection()
            .await
            .map_err(|error| AuthError::NonceStoreUnavailable(error.to_string()))?;
        let storage_key = nonce_storage_key(key_id, nonce);
        let ttl_seconds = replay_ttl_seconds(timestamp, replay_window);
        let set_result: Option<String> = redis::cmd("SET")
            .arg(&storage_key)
            .arg(timestamp.to_rfc3339())
            .arg("NX")
            .arg("EX")
            .arg(ttl_seconds)
            .query_async(&mut connection)
            .await
            .map_err(|error| AuthError::NonceStoreUnavailable(error.to_string()))?;

        if set_result.is_some() {
            Ok(())
        } else {
            Err(AuthError::NonceReused)
        }
    }
}

/// Returns a Redis-backed [NonceStore] when [redis_url] is set, falling back
/// to an in-memory store. Useful for backend code that needs single-use
/// nonce protection outside the SignedRequestVerifier hot path (e.g. the
/// device-pairing envelope nonce).
pub fn nonce_store_from_redis_url(
    redis_url: Option<&str>,
) -> Result<Arc<dyn NonceStore>, AuthError> {
    match redis_url {
        Some(url) if !url.is_empty() => Ok(Arc::new(RedisNonceStore::new(url)?)),
        _ => Ok(Arc::new(InMemoryNonceStore::default())),
    }
}

pub(super) fn nonce_storage_key(key_id: &str, nonce: &str) -> String {
    format!("{REDIS_NONCE_KEY_PREFIX}:{key_id}:{nonce}")
}

pub(super) fn replay_ttl_seconds(timestamp: DateTime<Utc>, replay_window: Duration) -> u64 {
    let ttl = (timestamp + replay_window - Utc::now())
        .num_seconds()
        .max(1);
    ttl as u64
}
