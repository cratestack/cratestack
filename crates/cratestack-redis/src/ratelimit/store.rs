use std::sync::Arc;

use cratestack_core::CoolError;
use redis::aio::ConnectionManager;
use sha2::{Digest, Sha256};
use tokio::sync::OnceCell;

use super::config::RedisRateLimitStoreConfig;
use super::util::{nibble_hex, redis_error};

#[derive(Clone)]
pub struct RedisRateLimitStore {
    pub(super) client: redis::Client,
    pub(super) config: RedisRateLimitStoreConfig,
    pub(super) conn: Arc<OnceCell<ConnectionManager>>,
}

impl RedisRateLimitStore {
    pub fn open(
        redis_url: impl redis::IntoConnectionInfo,
        key_prefix: impl Into<String>,
    ) -> Result<Self, CoolError> {
        let client = redis::Client::open(redis_url).map_err(redis_error)?;
        Ok(Self::from_client(client, key_prefix))
    }

    /// Opens a `rediss://` (TLS) connection, optionally trusting a private
    /// or internal CA instead of the system/webpki trust store.
    ///
    /// Requires the `tls-rustls` feature. Pass
    /// `redis::TlsCertificates { client_tls: None, root_cert: None }` to
    /// use the system trust store, or set `root_cert` to a PEM-encoded CA
    /// bundle to trust a private CA (e.g. behind a managed/HA Redis
    /// deployment that only exposes a TLS listener).
    #[cfg(feature = "tls-rustls")]
    pub fn open_with_tls(
        redis_url: impl redis::IntoConnectionInfo,
        key_prefix: impl Into<String>,
        tls_certs: redis::TlsCertificates,
    ) -> Result<Self, CoolError> {
        let client = redis::Client::build_with_tls(redis_url, tls_certs).map_err(redis_error)?;
        Ok(Self::from_client(client, key_prefix))
    }

    pub fn from_client(client: redis::Client, key_prefix: impl Into<String>) -> Self {
        Self {
            client,
            config: RedisRateLimitStoreConfig::new(key_prefix),
            conn: Arc::new(OnceCell::new()),
        }
    }

    pub fn key_prefix(&self) -> &str {
        &self.config.key_prefix
    }

    pub fn bucket_key(&self, key: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(key.as_bytes());
        let digest = hasher.finalize();
        let mut out = String::with_capacity(self.config.key_prefix.len() + 4 + 64);
        out.push_str(&self.config.key_prefix);
        out.push_str(":rl:");
        for byte in digest {
            out.push(nibble_hex(byte >> 4));
            out.push(nibble_hex(byte & 0x0f));
        }
        out
    }

    /// Returns a cheap clone of the shared, auto-reconnecting connection,
    /// establishing it once on first use rather than opening a new TCP
    /// connection to Redis on every call. A failed connection attempt is
    /// not cached, so the next call retries instead of failing forever.
    pub(super) async fn connection(&self) -> Result<ConnectionManager, CoolError> {
        let manager = self
            .conn
            .get_or_try_init(|| async { ConnectionManager::new(self.client.clone()).await })
            .await
            .map_err(redis_error)?;
        Ok(manager.clone())
    }
}
