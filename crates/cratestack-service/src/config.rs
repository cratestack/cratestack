//! Env-driven [`ServiceConfig`].

use std::net::SocketAddr;

use cratestack_core::CratestackError;

/// Configuration read from `{prefix}_*` environment variables.
///
/// Every variable name is built by prefixing the caller-supplied `prefix`
/// (e.g. `ServiceConfig::from_env("AUTH", "auth-service", 8080)` reads
/// `AUTH_SERVICE_HOST`, `AUTH_SERVICE_PORT`, `AUTH_DATABASE_URL`, ...).
/// This crate ships no default prefix, and — unlike the downstream code
/// this was absorbed from — no per-service defaults: no default connection
/// string, and no built-in service-name-to-database-name table. A caller
/// that wants a local-dev fallback sets it in their own environment (or
/// their own wrapper around `from_env`) before calling this.
///
/// `Debug` never prints a credential: the password of `database_url` and
/// `redis_url` and the whole of `object_storage_secret_key` are redacted.
#[derive(Clone)]
pub struct ServiceConfig {
    pub service_name: String,
    pub host: String,
    pub port: u16,
    /// Required (see [`ServiceConfig::from_env`]'s docs) when the
    /// `postgres` feature is enabled — this crate has no default
    /// connection string to fall back to.
    #[cfg(feature = "postgres")]
    pub database_url: String,
    pub redis_url: Option<String>,
    pub object_storage_endpoint: Option<String>,
    pub object_storage_public_url: Option<String>,
    pub object_storage_bucket: Option<String>,
    pub object_storage_access_key: Option<String>,
    pub object_storage_secret_key: Option<String>,
    pub public_base_url: String,
    env_prefix: String,
}

impl ServiceConfig {
    /// Read configuration from `{prefix}_*` environment variables.
    ///
    /// `default_port` is used only when `{prefix}_SERVICE_PORT` is unset
    /// or fails to parse as a `u16`. When the `postgres` feature is
    /// enabled, `{prefix}_DATABASE_URL` is required — every other
    /// variable is optional and falls back to a generic default (`0.0.0.0`
    /// for the host, `http://127.0.0.1:{port}` for the public base URL,
    /// `None` for everything else).
    pub fn from_env(
        prefix: &str,
        service_name: impl Into<String>,
        default_port: u16,
    ) -> Result<Self, CratestackError> {
        Self::from_env_with(prefix, service_name, default_port, |name: &str| {
            std::env::var(name)
        })
    }

    /// Same as [`ServiceConfig::from_env`], but variable lookups go
    /// through `lookup_env` instead of the real process environment — the
    /// same seam `cratestack-studio`'s `resolve_secret_with` uses, for the
    /// same reason: `std::env::set_var` is `unsafe` as of the 2024 edition
    /// (unsound against concurrent reads on other threads) and this
    /// workspace `forbid`s `unsafe_code`, so an injectable lookup is the
    /// only way to unit-test env parsing without process-wide mutation.
    pub(crate) fn from_env_with(
        prefix: &str,
        service_name: impl Into<String>,
        default_port: u16,
        lookup_env: impl Fn(&str) -> Result<String, std::env::VarError>,
    ) -> Result<Self, CratestackError> {
        let var = |suffix: &str| format!("{prefix}_{suffix}");
        let lookup = |suffix: &str| lookup_env(&var(suffix)).ok();

        let host = lookup("SERVICE_HOST").unwrap_or_else(|| "0.0.0.0".to_string());
        let port = lookup("SERVICE_PORT")
            .and_then(|value| value.parse::<u16>().ok())
            .unwrap_or(default_port);
        let public_base_url =
            lookup("PUBLIC_BASE_URL").unwrap_or_else(|| format!("http://127.0.0.1:{port}"));

        #[cfg(feature = "postgres")]
        let database_url = lookup("DATABASE_URL").ok_or_else(|| {
            CratestackError::Internal(format!("{} is required", var("DATABASE_URL")))
        })?;

        Ok(Self {
            service_name: service_name.into(),
            host,
            port,
            #[cfg(feature = "postgres")]
            database_url,
            redis_url: lookup("REDIS_URL"),
            object_storage_endpoint: lookup("OBJECT_STORAGE_ENDPOINT"),
            object_storage_public_url: lookup("OBJECT_STORAGE_PUBLIC_URL"),
            object_storage_bucket: lookup("OBJECT_STORAGE_BUCKET"),
            object_storage_access_key: lookup("OBJECT_STORAGE_ACCESS_KEY"),
            object_storage_secret_key: lookup("OBJECT_STORAGE_SECRET_KEY"),
            public_base_url,
            env_prefix: prefix.to_string(),
        })
    }

    pub fn bind_addr(&self) -> SocketAddr {
        format!("{}:{}", self.host, self.port)
            .parse()
            .expect("service bind address should be valid")
    }

    /// Reads `{prefix}_ENV`; `true` for `production` or `prod`.
    pub fn is_production(&self) -> bool {
        is_production_env(&self.env_prefix)
    }

    /// Fails with a descriptive [`CratestackError::Internal`] when
    /// `{prefix}_REDIS_URL` was not configured. Use this from a component
    /// that has an unconditional dependency on Redis (e.g. a Redis-backed
    /// idempotency store) while `redis_url` itself stays optional on
    /// [`ServiceConfig`] for components that don't.
    pub fn require_redis_url(&self, component: &str) -> Result<&str, CratestackError> {
        self.redis_url.as_deref().ok_or_else(|| {
            CratestackError::Internal(format!(
                "{component} requires {}_REDIS_URL to be configured",
                self.env_prefix
            ))
        })
    }

    /// Build the [`crate::ServiceState`] handed to `health` handlers: a
    /// lazily-connecting [`cratestack_sqlx::sqlx::PgPool`] (built by [`crate::PgPoolConfig`], max 5; real connection
    /// attempts happen on first use, e.g. inside
    /// [`crate::health::readiness`]) plus a clone of this config.
    #[cfg(feature = "postgres")]
    pub async fn state(&self) -> Result<crate::ServiceState, CratestackError> {
        let pool = cratestack_sqlx::PgPoolConfig::from_url(&self.database_url)
            .and_then(|config| config.max_connections(5).connect_lazy())
            .map_err(|error| {
                CratestackError::Internal(format!(
                    "invalid database url: {}",
                    error.public_message()
                ))
            })?;

        Ok(crate::ServiceState {
            config: self.clone(),
            pool,
        })
    }
}

/// Reads `{prefix}_ENV`; `true` for `production` or `prod`. Free function
/// (in addition to [`ServiceConfig::is_production`]) for callers that need
/// the answer before a [`ServiceConfig`] exists yet.
pub fn is_production_env(prefix: &str) -> bool {
    std::env::var(format!("{prefix}_ENV"))
        .map(|value| matches!(value.as_str(), "production" | "prod"))
        .unwrap_or(false)
}

mod redact;

#[cfg(test)]
mod tests;
