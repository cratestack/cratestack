use std::sync::Arc;
use std::time::Duration;

use cratestack_core::CratestackError;
use cratestack_core::log_throttle::LogThrottle;
use redis::aio::ConnectionManager;
use tokio::sync::OnceCell;

use crate::connection_config::manager_config;

use super::config::RedisRateLimitStoreConfig;
use super::util::{key_hash, redis_error};

/// How often the retry WARN may fire per store. Long enough that a
/// sustained outage cannot flood the log, short enough that an operator
/// watching a live incident still sees movement.
const RETRY_WARNING_INTERVAL: Duration = Duration::from_secs(10);

#[derive(Clone)]
pub struct RedisRateLimitStore {
    pub(super) client: redis::Client,
    pub(super) config: RedisRateLimitStoreConfig,
    pub(super) conn: Arc<OnceCell<ConnectionManager>>,
    /// Log budget for the retry WARN in `super::retry`. Per-store, not a
    /// `static`: see that function's docs.
    pub(super) retry_warning: Arc<LogThrottle>,
}

impl RedisRateLimitStore {
    pub fn open(
        redis_url: impl redis::IntoConnectionInfo,
        key_prefix: impl Into<String>,
    ) -> Result<Self, CratestackError> {
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
    ) -> Result<Self, CratestackError> {
        let client = redis::Client::build_with_tls(redis_url, tls_certs).map_err(redis_error)?;
        Ok(Self::from_client(client, key_prefix))
    }

    pub fn from_client(client: redis::Client, key_prefix: impl Into<String>) -> Self {
        Self {
            client,
            config: RedisRateLimitStoreConfig::new(key_prefix),
            conn: Arc::new(OnceCell::new()),
            retry_warning: Arc::new(LogThrottle::new(RETRY_WARNING_INTERVAL)),
        }
    }

    pub fn key_prefix(&self) -> &str {
        &self.config.key_prefix
    }

    pub fn bucket_key(&self, key: &str) -> String {
        self.namespaced(":rl:", &key_hash(key))
    }

    /// The Redis key holding one scope's distinct-bucket set
    /// (cratestack#871).
    ///
    /// **No window epoch in the key.** The first cut suffixed one, which
    /// meant every rollover minted a fresh set that re-admitted
    /// `max_distinct` more buckets while the previous generation was still
    /// alive — and made replicas with skewed clocks land on different
    /// generations at once. One key per scope, re-`PEXPIRE`d on every
    /// admission, is what actually bounds the keyspace; see
    /// `super::scripts`.
    ///
    /// A separate `:rls:` namespace, not `:rl:`, so a `SCAN <prefix>:rl:*`
    /// still counts buckets and only buckets — which is exactly what the
    /// regression test asserts a bound on.
    pub(super) fn scope_key(&self, scope: &str) -> String {
        self.namespaced(":rls:", &key_hash(scope))
    }

    fn namespaced(&self, infix: &str, suffix: &str) -> String {
        let mut out =
            String::with_capacity(self.config.key_prefix.len() + infix.len() + suffix.len());
        out.push_str(&self.config.key_prefix);
        out.push_str(infix);
        out.push_str(suffix);
        out
    }

    /// Returns a cheap clone of the shared, auto-reconnecting connection,
    /// establishing it once on first use rather than opening a new TCP
    /// connection to Redis on every call. A failed connection attempt is
    /// not cached, so the next call retries instead of failing forever.
    pub(super) async fn connection(&self) -> Result<ConnectionManager, CratestackError> {
        let manager = self
            .conn
            .get_or_try_init(|| async {
                ConnectionManager::new_with_config(self.client.clone(), manager_config()).await
            })
            .await
            .map_err(redis_error)?;
        Ok(manager.clone())
    }
}
