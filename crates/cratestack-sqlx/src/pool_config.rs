//! A bounded Postgres pool with session settings, built without naming sqlx.
//!
//! [`PgPoolConfig`] is the one public way to build the `PgPool` that
//! `Cratestack::builder(pool)` takes (cratestack#1385). A crate that uses
//! only generated `cratestack-pg` models can bound its pool and pin
//! `statement_timeout` / `lock_timeout` without depending on sqlx.
//!
//! ```rust,ignore
//! let config = cratestack::PgPoolConfig::from_url(&url)?   // BadRequest on a bad URL
//!     .max_connections(2)
//!     .acquire_timeout(Duration::from_secs(5))
//!     .test_before_acquire(true)
//!     .session_setting("statement_timeout", "30000")
//!     .session_setting("lock_timeout", "10000");
//! let pool = config.connect_lazy()?;
//! config.verify_settings(&pool).await?;                    // fail fast at start-up
//! ```
//!
//! # Session settings
//!
//! Each setting is sent as a startup `-c name=value` option, appended after
//! any `options` already in the URL, so it wins over a URL that sets the
//! same name (the server keeps the last `-c` for a name). A connection
//! pooler that drops startup options (pgbouncer in transaction mode) defeats
//! this silently; [`PgPoolConfig::verify_settings`] is how to notice.
//!
//! Five names are excluded: `DateStyle`, `client_encoding`, `TimeZone`,
//! `extra_float_digits` and `application_name`. sqlx sends them itself as
//! startup parameters, which the server applies after `-c` options, so a
//! `-c` for them would silently lose. `session_setting` refuses them
//! (`BadRequest`, case-insensitive); [`PgPoolConfig::application_name`] is the
//! supported way to set the one that is useful. The same five names in a
//! URL's own `options=` (`-c TimeZone=UTC`, `-cdatestyle=ISO`, `--name=v`) are
//! refused by `from_url` with `BadRequest`, for the same reason.
//!
//! # URL forms
//!
//! Only URLs are accepted (`postgres://` or `postgresql://`). A libpq
//! key=value DSN (`host=localhost dbname=app`) is refused with a
//! `BadRequest` that says so, rather than reaching the URL parser and
//! failing with a message about something else. Convert it to a URL.
//!
//! # No separate connect timeout
//!
//! sqlx has no connect-timeout knob. Establishing a connection happens
//! inside `acquire`, so [`PgPoolConfig::acquire_timeout`] bounds the wait
//! for a free connection *and* for a new one to come up.

use std::fmt;
use std::time::Duration;

use cratestack_core::CratestackError;

use crate::sqlx::postgres::PgConnectOptions;

mod connect;
mod validate;
mod verify;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_classify;

/// Pool size when [`PgPoolConfig::max_connections`] is not called. Matches
/// what `cratestack-service`'s `ServiceConfig::state` always used.
const DEFAULT_MAX_CONNECTIONS: u32 = 5;

/// Builder for a bounded Postgres pool with session settings.
///
/// See the [module docs](self). `Debug` omits the connection details, so a
/// logged config never carries the URL's password.
#[derive(Clone)]
pub struct PgPoolConfig {
    connect: PgConnectOptions,
    max_connections: u32,
    acquire_timeout: Option<Duration>,
    test_before_acquire: Option<bool>,
    /// Name to value, in the order set; a repeated name keeps its last value.
    settings: Vec<(String, String)>,
    application_name: Option<String>,
    /// First invalid input seen by an infallible setter, reported when the
    /// pool is built so the chain stays a plain builder.
    invalid: Option<String>,
}

impl fmt::Debug for PgPoolConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PgPoolConfig")
            .field("max_connections", &self.max_connections)
            .field("acquire_timeout", &self.acquire_timeout)
            .field("test_before_acquire", &self.test_before_acquire)
            .field("settings", &self.settings)
            .finish_non_exhaustive()
    }
}

impl PgPoolConfig {
    /// Start from a `postgres://` or `postgresql://` URL.
    ///
    /// A bad URL, or a key=value DSN, is `CratestackError::BadRequest`. The
    /// message never echoes any part of the input, so a password in a
    /// malformed URL cannot reach a log.
    pub fn from_url(url: &str) -> Result<Self, CratestackError> {
        Ok(Self {
            connect: validate::parse_url(url)?,
            max_connections: DEFAULT_MAX_CONNECTIONS,
            acquire_timeout: None,
            test_before_acquire: None,
            settings: Vec::new(),
            application_name: None,
            invalid: None,
        })
    }

    /// Upper bound on open connections (default 5). Zero is refused when the
    /// pool is built.
    pub fn max_connections(mut self, max: u32) -> Self {
        self.max_connections = max;
        self
    }

    /// How long a caller waits for a connection, including the time to open
    /// a new one. sqlx's default (30 s) applies when unset.
    pub fn acquire_timeout(mut self, timeout: Duration) -> Self {
        self.acquire_timeout = Some(timeout);
        self
    }

    /// Ping a pooled connection before handing it out. sqlx's default
    /// applies when unset.
    pub fn test_before_acquire(mut self, test: bool) -> Self {
        self.test_before_acquire = Some(test);
        self
    }

    /// Set a Postgres setting on every connection at startup, for example
    /// `("statement_timeout", "30000")`.
    ///
    /// `name` must match `[a-z_][a-z0-9_.]*` (a custom `app.name` setting is
    /// fine) and `value` must hold no control character; anything else is a
    /// `BadRequest` reported when the pool is built. A name set twice keeps
    /// its last value. `DateStyle`, `client_encoding`, `TimeZone`,
    /// `extra_float_digits` and `application_name` are refused (any case):
    /// sqlx sets them at startup and the server applies those after `-c`
    /// options. Use [`application_name`](Self::application_name) for the last.
    pub fn session_setting(mut self, name: &str, value: &str) -> Self {
        if let Err(message) = validate::check_setting(name, value) {
            self.invalid.get_or_insert(message);
            return self;
        }
        self.settings.retain(|(existing, _)| existing != name);
        self.settings.push((name.to_owned(), value.to_owned()));
        self
    }

    /// Set `application_name` on every connection, through sqlx's own
    /// startup parameter (the only way it reliably applies). A value with a
    /// control character is a `BadRequest` reported when the pool is built.
    pub fn application_name(mut self, name: &str) -> Self {
        if name.chars().any(char::is_control) {
            self.invalid
                .get_or_insert("application_name has a control character in its value".to_owned());
            return self;
        }
        self.application_name = Some(name.to_owned());
        self
    }
}
