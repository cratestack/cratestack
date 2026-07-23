use cratestack_core::CoolError;
use sha2::{Digest, Sha256};

use super::config::RedisIdempotencyStoreConfig;
use super::util::{nibble_hex, redis_error};

#[derive(Clone)]
pub struct RedisIdempotencyStore {
    pub(super) client: redis::Client,
    pub(super) config: RedisIdempotencyStoreConfig,
}

impl RedisIdempotencyStore {
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
            config: RedisIdempotencyStoreConfig::new(key_prefix),
        }
    }

    pub fn key_prefix(&self) -> &str {
        &self.config.key_prefix
    }

    pub fn hash_key(&self, principal: &str, key: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(principal.as_bytes());
        hasher.update([0u8]);
        hasher.update(key.as_bytes());
        let digest = hasher.finalize();
        let mut out = String::with_capacity(self.config.key_prefix.len() + 6 + 64);
        out.push_str(&self.config.key_prefix);
        out.push_str(":idem:");
        for byte in digest {
            out.push(nibble_hex(byte >> 4));
            out.push(nibble_hex(byte & 0x0f));
        }
        out
    }

    pub(super) async fn connection(&self) -> Result<redis::aio::MultiplexedConnection, CoolError> {
        self.client
            .get_multiplexed_async_connection()
            .await
            .map_err(redis_error)
    }
}
