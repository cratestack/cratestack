//! Redis-backed [`NonceStore`], and the public factory that picks between
//! it and the in-memory default based on configuration.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use redis::Client as RedisClient;

use super::consts::REDIS_NONCE_KEY_PREFIX;
use super::nonce_redis_conn::{SharedConnection, dropped_before_running, is_plain_server_reply};
use super::nonce_store::{InMemoryNonceStore, NonceStore};
use crate::AuthError;

/// Why a `SET NX` produced no answer. Every variant is answered
/// `NonceStoreUnavailable`: an error is never "new".
enum SetFailure {
    /// No connection was made. The attempt used the whole 1 s connect
    /// budget (or was refused at once) and is not repeated.
    Connect(String),
    /// The connection failed or timed out; it has been discarded.
    Command(redis::RedisError),
    /// The server answered with an error reply. The connection stays.
    Reply(redis::RedisError),
}

pub(super) struct RedisNonceStore {
    /// The shared connection, opened by the first claim and rebuilt after
    /// any failure that is not a server reply.
    connection: SharedConnection,
}

impl RedisNonceStore {
    pub(super) fn new(redis_url: &str) -> Result<Self, AuthError> {
        let client = RedisClient::open(redis_url).map_err(|error| {
            AuthError::InvalidNonceStoreConfiguration(format!(
                "invalid redis url for nonce store: {error}"
            ))
        })?;
        Ok(Self {
            connection: SharedConnection::new(client),
        })
    }

    /// `SET key value NX EX ttl`: `Some` when the key was stored, `None`
    /// when it already existed (a replay).
    async fn set_nx(
        &self,
        storage_key: &str,
        value: &str,
        ttl_seconds: u64,
    ) -> Result<Option<String>, SetFailure> {
        let attempt = self.connection.attempt();
        let mut connection = self
            .connection
            .manager(&attempt)
            .await
            .map_err(SetFailure::Connect)?;
        let result = redis::cmd("SET")
            .arg(storage_key)
            .arg(value)
            .arg("NX")
            .arg("EX")
            .arg(ttl_seconds)
            .query_async(&mut connection)
            .await;
        result.map_err(|error| {
            if is_plain_server_reply(&error) {
                SetFailure::Reply(error)
            } else {
                self.connection.reset(&attempt);
                SetFailure::Command(error)
            }
        })
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
        let storage_key = nonce_storage_key(key_id, nonce);
        let ttl_seconds = replay_ttl_seconds(timestamp, replay_window);
        let value = timestamp.to_rfc3339();
        let set_result = match self.set_nx(&storage_key, &value, ttl_seconds).await {
            // Retried once, and only when the connection was already gone
            // or broke on the write, so the command cannot have been
            // waiting behind a slow server. The connection was discarded,
            // so the retry connects afresh. If the first SET did land, the
            // retry finds the key and answers NonceReused: that fails
            // closed. Never retried on a timeout (the retry would queue
            // behind the original SET and turn a first use into a replay)
            // or on a failed connect (that attempt used the claim's budget).
            Err(SetFailure::Command(error)) if dropped_before_running(&error) => {
                self.set_nx(&storage_key, &value, ttl_seconds).await
            }
            other => other,
        }
        .map_err(|failure| {
            AuthError::NonceStoreUnavailable(match failure {
                SetFailure::Connect(error) => error,
                SetFailure::Command(error) | SetFailure::Reply(error) => error.to_string(),
            })
        })?;

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
