use cratestack_core::CoolError;
use sha2::{Digest, Sha256};

use super::config::RedisRateLimitStoreConfig;
use super::util::{nibble_hex, redis_error};

#[derive(Clone)]
pub struct RedisRateLimitStore {
    pub(super) client: redis::Client,
    pub(super) config: RedisRateLimitStoreConfig,
}

impl RedisRateLimitStore {
    pub fn open(
        redis_url: impl redis::IntoConnectionInfo,
        key_prefix: impl Into<String>,
    ) -> Result<Self, CoolError> {
        let client = redis::Client::open(redis_url).map_err(redis_error)?;
        Ok(Self::from_client(client, key_prefix))
    }

    pub fn from_client(client: redis::Client, key_prefix: impl Into<String>) -> Self {
        Self {
            client,
            config: RedisRateLimitStoreConfig::new(key_prefix),
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

    pub(super) async fn connection(
        &self,
    ) -> Result<redis::aio::MultiplexedConnection, CoolError> {
        self.client
            .get_multiplexed_async_connection()
            .await
            .map_err(redis_error)
    }
}
