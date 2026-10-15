//! Redis-backed [`NonceStore`], and the public factory that picks between
//! it and the in-memory default based on configuration.

use std::sync::Arc;
use std::time::Duration as StdDuration;

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use redis::Client as RedisClient;
use redis::aio::{ConnectionManager, ConnectionManagerConfig};
use tokio::sync::OnceCell;

use super::consts::REDIS_NONCE_KEY_PREFIX;
use super::nonce_store::{InMemoryNonceStore, NonceStore};
use crate::AuthError;

/// The driver's own defaults, made explicit: one connect attempt gets 1 s
/// and one reply 500 ms. A Redis outage therefore still answers
/// `NonceStoreUnavailable` about a second in, as it did before the store
/// held a connection (#1070). Deliberately not the 2 s used by
/// `cratestack-redis`: this path is on every signed request.
const CONNECT_TIMEOUT: StdDuration = StdDuration::from_secs(1);
const RESPONSE_TIMEOUT: StdDuration = StdDuration::from_millis(500);

pub(super) struct RedisNonceStore {
    client: RedisClient,
    /// The one managed connection, opened by the first claim and shared by
    /// every later one. It reconnects by itself after a dropped connection.
    connection: OnceCell<ConnectionManager>,
}

impl RedisNonceStore {
    pub(super) fn new(redis_url: &str) -> Result<Self, AuthError> {
        let client = RedisClient::open(redis_url).map_err(|error| {
            AuthError::InvalidNonceStoreConfiguration(format!(
                "invalid redis url for nonce store: {error}"
            ))
        })?;
        Ok(Self {
            client,
            connection: OnceCell::new(),
        })
    }

    /// Returns a cheap clone of the shared connection, opening it on first
    /// use. A failed attempt is not cached, so the next claim retries.
    async fn connection(&self) -> redis::RedisResult<ConnectionManager> {
        let manager = self
            .connection
            .get_or_try_init(|| async {
                ConnectionManager::new_with_config(self.client.clone(), manager_config()).await
            })
            .await?;
        Ok(manager.clone())
    }

    /// `SET key value NX EX ttl`: `Some` when the key was stored, `None`
    /// when it already existed (a replay).
    async fn set_nx(
        &self,
        storage_key: &str,
        value: &str,
        ttl_seconds: u64,
    ) -> redis::RedisResult<Option<String>> {
        let mut connection = self.connection().await?;
        redis::cmd("SET")
            .arg(storage_key)
            .arg(value)
            .arg("NX")
            .arg("EX")
            .arg(ttl_seconds)
            .query_async(&mut connection)
            .await
    }
}

/// No reconnect backoff: the driver's default of six retries makes a claim
/// wait out roughly nine seconds of an outage, on the request path. With
/// none, an outage fails the claim in milliseconds (fail closed), and the
/// next claim makes one fresh attempt, so recovery needs no restart.
fn manager_config() -> ConnectionManagerConfig {
    ConnectionManagerConfig::new()
        .set_connection_timeout(Some(CONNECT_TIMEOUT))
        .set_response_timeout(Some(RESPONSE_TIMEOUT))
        .set_number_of_retries(0)
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
        let storage_key = nonce_storage_key(key_id, nonce);
        let ttl_seconds = replay_ttl_seconds(timestamp, replay_window);
        let value = timestamp.to_rfc3339();
        let set_result = match self.set_nx(&storage_key, &value, ttl_seconds).await {
            // The shared connection's first command after the server dropped
            // it fails before the manager reconnects, so retry once. Safe
            // because SET NX is atomic: if a lost reply hid a write that did
            // happen, the retry finds the key and answers NonceReused.
            Err(error) if error.is_connection_dropped() || error.is_io_error() => {
                self.set_nx(&storage_key, &value, ttl_seconds).await
            }
            other => other,
        }
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
