//! Building the pool: validation, options, `connect_lazy`, `connect` and how
//! its failures are classified.

use cratestack_core::CratestackError;

use super::PgPoolConfig;
use crate::cratestack_error_from_sqlx;
use crate::sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use crate::sqlx::{self, PgPool};

impl PgPoolConfig {
    /// Everything checkable without a server; no connection options built.
    pub(super) fn validate(&self) -> Result<(), CratestackError> {
        if let Some(message) = &self.invalid {
            return Err(CratestackError::BadRequest(message.clone()));
        }
        if self.max_connections == 0 {
            return Err(CratestackError::BadRequest(
                "max_connections must be at least 1".to_owned(),
            ));
        }
        Ok(())
    }

    /// The connection options with the session settings appended last.
    pub(crate) fn connect_options(&self) -> Result<PgConnectOptions, CratestackError> {
        self.validate()?;
        let mut options = self.connect.clone().options(
            self.settings
                .iter()
                .map(|(name, value)| (name.as_str(), value.as_str())),
        );
        if let Some(name) = &self.application_name {
            options = options.application_name(name);
        }
        Ok(options)
    }

    pub(super) fn pool_options(&self) -> PgPoolOptions {
        let mut options = PgPoolOptions::new().max_connections(self.max_connections);
        if let Some(timeout) = self.acquire_timeout {
            options = options.acquire_timeout(timeout);
        }
        if let Some(test) = self.test_before_acquire {
            options = options.test_before_acquire(test);
        }
        options
    }

    /// Build the pool without connecting; the first connection is made on
    /// first use. Needs a Tokio runtime. The result is the `PgPool` that
    /// `Cratestack::builder` takes.
    ///
    /// Fails with `BadRequest` on an invalid setting name or value, or a
    /// zero `max_connections`.
    pub fn connect_lazy(&self) -> Result<PgPool, CratestackError> {
        Ok(self
            .pool_options()
            .connect_lazy_with(self.connect_options()?))
    }

    /// Build the pool and open one connection now.
    ///
    /// Failures are classified, and no message echoes the URL (the cause is
    /// logged, not returned, since it can carry the host and port):
    /// - `BadRequest`: a configuration fault that retrying cannot fix. An
    ///   invalid role or database (SQLSTATE class `28`, `3D`), an unknown or
    ///   unsettable setting or an invalid value (`42704`, `22xxx`, `42xxx`,
    ///   `55P02`, which includes a `postmaster`/`sighup` setting), a TLS
    ///   negotiation failure, or an invalid sqlx configuration.
    /// - `Unavailable`: the database could not be reached (`Io`,
    ///   `PoolTimedOut`, `PoolClosed`, `WorkerCrashed`) or is going away or
    ///   restarting (SQLSTATE `57P01`, `57P02`, `57P03`, class `08`).
    /// - Anything else goes through [`cratestack_error_from_sqlx`].
    pub async fn connect(&self) -> Result<PgPool, CratestackError> {
        let options = self.connect_options()?;
        self.pool_options()
            .connect_with(options)
            .await
            .map_err(classify_connect_error)
    }
}

pub(super) fn classify_connect_error(error: sqlx::Error) -> CratestackError {
    tracing::warn!(%error, "postgres pool could not connect");
    let bad = |message: &str| CratestackError::BadRequest(message.to_owned());
    match &error {
        sqlx::Error::Database(db) => match db.code().as_deref() {
            Some(code) if code.starts_with("28") => {
                bad("database rejected the role (invalid authorization)")
            }
            Some(code) if code.starts_with("3D") => {
                bad("database does not exist (invalid catalog name)")
            }
            Some("55P02") => bad("a configured session setting cannot be set at connection start"),
            Some("42704") => {
                bad("unknown setting: a configured session setting is not known to the server")
            }
            Some(code) if code.starts_with("22") || code.starts_with("42") => {
                bad("the server rejected a configured session setting or its value")
            }
            // Transient: the server is shutting down or restarting (57P01 admin
            // shutdown, 57P02 crash shutdown, 57P03 cannot connect now) or the
            // connection broke (class 08). Retrying can succeed.
            Some(code) if matches!(code, "57P01" | "57P02" | "57P03") || code.starts_with("08") => {
                CratestackError::Unavailable("database unavailable".to_owned())
            }
            // 55000 (object not in prerequisite state, e.g. `allow_connections
            // false`) is deliberately not listed: it is an operator-set state
            // that does not clear by retrying, so it keeps the generic mapping.
            _ => cratestack_error_from_sqlx(error),
        },
        sqlx::Error::Configuration(_) => bad("database connection configuration is invalid"),
        sqlx::Error::Tls(_) => bad("TLS negotiation failed"),
        sqlx::Error::Io(_)
        | sqlx::Error::PoolTimedOut
        | sqlx::Error::PoolClosed
        | sqlx::Error::WorkerCrashed => {
            CratestackError::Unavailable("database unavailable".to_owned())
        }
        _ => cratestack_error_from_sqlx(error),
    }
}
