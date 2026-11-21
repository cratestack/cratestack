//! Typed conversion from `sqlx::Error` to `CratestackError`.
//!
//! The central entry point is [`cratestack_error_from_sqlx`], which should be used
//! instead of `CratestackError::Database(error.to_string())` at every sqlx call
//! site.  When the underlying error is `sqlx::Error::Database`, the
//! structured fields (`code`, `constraint`, `table`, `column`, `schema`) are captured in a
//! [`DbErrorInfo`][cratestack_core::DbErrorInfo] and stored in the
//! [`CratestackError::DatabaseTyped`] variant so consumers can call
//! [`CratestackError::db_sqlstate`] / [`CratestackError::db_constraint`] /
//! [`CratestackError::db_table`] instead of
//! substring-matching the stringified detail.
//!
//! Connection-class errors (`PoolTimedOut`, `PoolClosed`, `WorkerCrashed`,
//! `Io`) are mapped to `CratestackError::Unavailable` (HTTP 503), so a
//! caller can tell "could not reach the database" from a protocol or decode
//! failure, which stays `Database`. `Configuration` and `Tls` errors stay
//! `Database` too but with a fixed message: sqlx's text for them can carry
//! the connection URL and password, so the cause goes to tracing only.
//!
//! `sqlx::Error::RowNotFound` is mapped to `CratestackError::NotFound` so a missing
//! row surfaces as a 404 rather than a 500. Callers that want a custom
//! not-found message should construct it themselves before calling this
//! helper.

use cratestack_core::{CratestackError, DbErrorInfo};

use crate::sqlx;

/// Convert a `sqlx::Error` to `CratestackError`, preserving structured database
/// error information when available.
///
/// # When a typed variant is produced
///
/// If `error` is `sqlx::Error::Database(db_err)`, this function produces
/// `CratestackError::DatabaseTyped` with the SQLSTATE code and constraint name
/// extracted from the driver error.  Connection-class errors
/// (`PoolTimedOut`, `PoolClosed`, `WorkerCrashed`, `Io`) become
/// `CratestackError::Unavailable`. All other `sqlx::Error` kinds (decode
/// errors, protocol errors, etc.) fall back to `CratestackError::Database` with the
/// stringified message, identical to the legacy `error.to_string()` path.
///
/// # Usage
///
/// ```rust,ignore
/// use cratestack_sqlx::cratestack_error_from_sqlx;
///
/// sqlx::query("INSERT …")
///     .execute(&pool)
///     .await
///     .map_err(cratestack_error_from_sqlx)?;
/// ```
///
/// Consumers can then inspect the error:
///
/// ```rust,ignore
/// if err.db_sqlstate() == Some("23505") {
///     let constraint = err.db_constraint(); // e.g. "accounts_email_key"
/// }
/// ```
pub fn cratestack_error_from_sqlx(error: sqlx::Error) -> CratestackError {
    match error {
        sqlx::Error::Database(db_err) => CratestackError::DatabaseTyped(db_error_info(&*db_err)),
        sqlx::Error::RowNotFound => CratestackError::NotFound("not found".to_owned()),
        // The database could not be reached or answered at all. `Unavailable`'s
        // string is the public message, so it is fixed: the cause (an `Io`
        // error can carry the host and port) goes to tracing only.
        error @ (sqlx::Error::PoolTimedOut
        | sqlx::Error::PoolClosed
        | sqlx::Error::WorkerCrashed
        | sqlx::Error::Io(_)) => {
            tracing::warn!(%error, "database unavailable");
            CratestackError::Unavailable("database unavailable".to_owned())
        }
        // A connection string that does not parse or configure, or a TLS
        // setup that failed: both are permanent faults (not retryable), and
        // sqlx's text for them can carry the connection URL, password
        // included, or the host. The result carries a fixed message only;
        // the cause goes to tracing.
        error @ sqlx::Error::Configuration(_) => {
            tracing::error!(%error, "database connection configuration error");
            CratestackError::Database("database connection configuration error".to_owned())
        }
        error @ sqlx::Error::Tls(_) => {
            tracing::error!(%error, "database TLS error");
            CratestackError::Database("database TLS error".to_owned())
        }
        other => CratestackError::Database(other.to_string()),
    }
}

/// The one place a driver `DatabaseError` becomes a [`DbErrorInfo`], shared
/// by [`cratestack_error_from_sqlx`] and the unique-violation classifier so
/// both paths report the same detail, SQLSTATE, constraint, table, column
/// and schema. The detail is the server's message text (`message()`), not
/// `to_string()`, so a driver suffix such as ` at line N` is not appended.
pub(crate) fn db_error_info(db_err: &dyn sqlx::error::DatabaseError) -> DbErrorInfo {
    let mut info = DbErrorInfo::new(db_err.message());
    if let Some(code) = db_err.code() {
        info = info.with_sqlstate(code);
    }
    if let Some(constraint) = db_err.constraint() {
        info = info.with_constraint(constraint);
    }
    if let Some(table) = db_err.table() {
        info = info.with_table(table);
    }
    if let Some(pg) = db_err.try_downcast_ref::<sqlx::postgres::PgDatabaseError>() {
        if let Some(column) = pg.column() {
            info = info.with_column(column);
        }
        if let Some(schema) = pg.schema() {
            info = info.with_schema(schema);
        }
    }
    info
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_redaction;
