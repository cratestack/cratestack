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
#[derive(Clone, Debug)]
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
    /// lazily-connecting [`cratestack_sqlx::sqlx::PgPool`] (real connection
    /// attempts happen on first use, e.g. inside
    /// [`crate::health::readiness`]) plus a clone of this config.
    #[cfg(feature = "postgres")]
    pub async fn state(&self) -> Result<crate::ServiceState, CratestackError> {
        use cratestack_sqlx::sqlx::postgres::{PgConnectOptions, PgPoolOptions};

        let options: PgConnectOptions = self
            .database_url
            .parse()
            .map_err(|error| CratestackError::Internal(format!("invalid database url: {error}")))?;
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect_lazy_with(options);

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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::ServiceConfig;

    fn lookup_from(
        vars: &'static [(&'static str, &'static str)],
    ) -> impl Fn(&str) -> Result<String, std::env::VarError> {
        let map: HashMap<&'static str, &'static str> = vars.iter().copied().collect();
        move |name: &str| {
            map.get(name)
                .map(|value| value.to_string())
                .ok_or(std::env::VarError::NotPresent)
        }
    }

    #[test]
    fn env_prefix_is_honoured() {
        let lookup = lookup_from(&[
            ("AUTH_SERVICE_HOST", "10.0.0.5"),
            ("AUTH_SERVICE_PORT", "9090"),
            ("AUTH_DATABASE_URL", "postgres://a/b"),
            ("AUTH_PUBLIC_BASE_URL", "https://auth.example.com"),
            ("AUTH_REDIS_URL", "redis://localhost:6379"),
            // A same-suffix variable under a DIFFERENT prefix must be
            // ignored — proves the check isn't accidentally matching on
            // the bare suffix.
            ("CATALOG_SERVICE_HOST", "should-not-be-picked-up"),
        ]);

        let config = ServiceConfig::from_env_with("AUTH", "auth-service", 8080, &lookup)
            .expect("all required vars are present");

        assert_eq!(config.host, "10.0.0.5");
        assert_eq!(config.port, 9090);
        assert_eq!(config.public_base_url, "https://auth.example.com");
        assert_eq!(config.redis_url.as_deref(), Some("redis://localhost:6379"));
        #[cfg(feature = "postgres")]
        assert_eq!(config.database_url, "postgres://a/b");
    }

    #[test]
    fn falls_back_to_generic_defaults_when_unset() {
        let lookup = lookup_from(&[("CATALOG_DATABASE_URL", "postgres://c/d")]);

        let config = ServiceConfig::from_env_with("CATALOG", "catalog-service", 8083, &lookup)
            .expect("database url present");

        assert_eq!(config.host, "0.0.0.0");
        assert_eq!(config.port, 8083);
        assert_eq!(config.public_base_url, "http://127.0.0.1:8083");
        assert_eq!(config.redis_url, None);
        assert_eq!(config.object_storage_endpoint, None);
    }

    #[cfg(feature = "postgres")]
    #[test]
    fn missing_database_url_is_an_error() {
        let lookup = lookup_from(&[]);

        let error = ServiceConfig::from_env_with("VENDOR", "vendor-service", 8084, &lookup)
            .expect_err("no VENDOR_DATABASE_URL was provided");

        assert!(matches!(
            error,
            cratestack_core::CratestackError::Internal(_)
        ));
        assert!(error.to_string().contains("VENDOR_DATABASE_URL"));
    }

    #[test]
    fn require_redis_url_reports_the_component_and_prefix() {
        // `SEARCH_DATABASE_URL` is present even when the `postgres` feature
        // is off, where it is simply never looked up — harmless either way.
        let lookup = lookup_from(&[("SEARCH_DATABASE_URL", "postgres://s/t")]);
        let config = ServiceConfig::from_env_with("SEARCH", "search-service", 8085, &lookup)
            .expect("required vars present");

        let error = config
            .require_redis_url("rate limiter")
            .expect_err("SEARCH_REDIS_URL was never set");

        let message = error.to_string();
        assert!(message.contains("rate limiter"));
        assert!(message.contains("SEARCH_REDIS_URL"));
    }
}
