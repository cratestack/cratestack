//! Typed conversion from `sqlx::Error` to `CratestackError`.
//!
//! The central entry point is [`cratestack_error_from_sqlx`], which should be used
//! instead of `CratestackError::Database(error.to_string())` at every sqlx call
//! site.  When the underlying error is `sqlx::Error::Database`, the
//! structured fields (`code`, `constraint`) are captured in a
//! [`DbErrorInfo`][cratestack_core::DbErrorInfo] and stored in the
//! [`CratestackError::DatabaseTyped`] variant so consumers can call
//! [`CratestackError::db_sqlstate`] / [`CratestackError::db_constraint`] instead of
//! substring-matching the stringified detail.
//!
//! Connection-class errors (`PoolTimedOut`, `PoolClosed`, `WorkerCrashed`,
//! `Io`) are mapped to `CratestackError::Unavailable` (HTTP 503), so a
//! caller can tell "could not reach the database" from a protocol or decode
//! failure, which stays `Database`.
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
        sqlx::Error::Database(db_err) => {
            let detail = db_err.to_string();
            let sqlstate = db_err.code().map(|c| c.into_owned());
            let constraint = db_err.constraint().map(ToOwned::to_owned);
            CratestackError::DatabaseTyped(DbErrorInfo {
                detail,
                sqlstate,
                constraint,
            })
        }
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
        other => CratestackError::Database(other.to_string()),
    }
}

#[cfg(test)]
mod tests;
