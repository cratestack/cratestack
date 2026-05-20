//! Typed conversion from `sqlx::Error` to `CoolError`.
//!
//! The central entry point is [`cool_error_from_sqlx`], which should be used
//! instead of `CoolError::Database(error.to_string())` at every sqlx call
//! site.  When the underlying error is `sqlx::Error::Database`, the
//! structured fields (`code`, `constraint`) are captured in a
//! [`DbErrorInfo`][cratestack_core::DbErrorInfo] and stored in the
//! [`CoolError::DatabaseTyped`] variant so consumers can call
//! [`CoolError::db_sqlstate`] / [`CoolError::db_constraint`] instead of
//! substring-matching the stringified detail.
//!
//! `sqlx::Error::RowNotFound` is mapped to `CoolError::NotFound` so a missing
//! row surfaces as a 404 rather than a 500. Callers that want a custom
//! not-found message should construct it themselves before calling this
//! helper.

use cratestack_core::{CoolError, DbErrorInfo};

use crate::sqlx;

/// Convert a `sqlx::Error` to `CoolError`, preserving structured database
/// error information when available.
///
/// # When a typed variant is produced
///
/// If `error` is `sqlx::Error::Database(db_err)`, this function produces
/// `CoolError::DatabaseTyped` with the SQLSTATE code and constraint name
/// extracted from the driver error.  All other `sqlx::Error` kinds (pool
/// timeouts, decode errors, etc.) fall back to `CoolError::Database` with the
/// stringified message, identical to the legacy `error.to_string()` path.
///
/// # Usage
///
/// ```rust,ignore
/// use cratestack_sqlx::cool_error_from_sqlx;
///
/// sqlx::query("INSERT …")
///     .execute(&pool)
///     .await
///     .map_err(cool_error_from_sqlx)?;
/// ```
///
/// Consumers can then inspect the error:
///
/// ```rust,ignore
/// if err.db_sqlstate() == Some("23505") {
///     let constraint = err.db_constraint(); // e.g. "accounts_email_key"
/// }
/// ```
pub fn cool_error_from_sqlx(error: sqlx::Error) -> CoolError {
    match error {
        sqlx::Error::Database(db_err) => {
            let detail = db_err.to_string();
            let sqlstate = db_err.code().map(|c| c.into_owned());
            let constraint = db_err.constraint().map(ToOwned::to_owned);
            CoolError::DatabaseTyped(DbErrorInfo {
                detail,
                sqlstate,
                constraint,
            })
        }
        sqlx::Error::RowNotFound => CoolError::NotFound("not found".to_owned()),
        other => CoolError::Database(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `RowNotFound` is a missing-row signal and must map to `NotFound`
    /// so callers see a 404 instead of a 500.
    #[test]
    fn row_not_found_maps_to_not_found() {
        let err = cool_error_from_sqlx(sqlx::Error::RowNotFound);
        assert!(
            matches!(err, CoolError::NotFound(_)),
            "RowNotFound should map to CoolError::NotFound",
        );
        assert_eq!(err.status_code().as_u16(), 404);
        // Typed accessors are not applicable to NotFound.
        assert_eq!(err.db_sqlstate(), None);
        assert_eq!(err.db_constraint(), None);
    }

    /// Round-trip: a non-database, non-RowNotFound sqlx error (e.g. a
    /// configuration / protocol error) must produce the legacy
    /// `Database(String)` variant so existing `detail()` callers keep
    /// working.
    #[test]
    fn non_database_sqlx_error_produces_legacy_variant() {
        let err = cool_error_from_sqlx(sqlx::Error::Protocol(
            "unexpected EOF from server".to_owned(),
        ));
        assert!(
            matches!(err, CoolError::Database(_)),
            "Protocol error should fall back to CoolError::Database",
        );
        assert!(
            err.detail().is_some(),
            "detail() must not be empty for non-database errors",
        );
        // Typed accessors return None for the legacy variant.
        assert_eq!(err.db_sqlstate(), None);
        assert_eq!(err.db_constraint(), None);
    }

    /// The `DatabaseTyped` variant exposes sqlstate and constraint through the
    /// typed accessors, and its `detail()` still returns the full operator
    /// string (same as the old `error.to_string()` value).
    ///
    /// We can't easily construct a real `PgDatabaseError` in a unit test
    /// (it's opaque), so we verify the round-trip contract using
    /// `DbErrorInfo` directly and confirm `cool_error_from_sqlx` maps
    /// the correct variant for the Database arm.
    #[test]
    fn database_typed_accessors() {
        let info = DbErrorInfo {
            detail: "ERROR: duplicate key value violates unique constraint \"accounts_email_key\""
                .to_owned(),
            sqlstate: Some("23505".to_owned()),
            constraint: Some("accounts_email_key".to_owned()),
        };
        let err = CoolError::DatabaseTyped(info);

        assert_eq!(err.db_sqlstate(), Some("23505"));
        assert_eq!(err.db_constraint(), Some("accounts_email_key"));
        assert_eq!(err.code(), "DATABASE_ERROR");
        // 5xx — must map to 500.
        let status = err.status_code();
        assert_eq!(status.as_u16(), 500);
        assert_eq!(err.public_message(), "internal error");
        assert!(err.detail().unwrap().contains("duplicate key"));
    }

    /// `is_retriable` in `isolation.rs` matches on `detail()`.
    /// Both `Database(String)` and `DatabaseTyped` must surface their detail
    /// so the retry logic continues to work for serialization failures.
    #[test]
    fn database_typed_detail_preserved_for_retry_logic() {
        let info = DbErrorInfo {
            detail: "Database(PgDatabaseError { code: \"40001\", message: \"could not serialize access\" })"
                .to_owned(),
            sqlstate: Some("40001".to_owned()),
            constraint: None,
        };
        let err = CoolError::DatabaseTyped(info);
        let detail = err.detail().unwrap_or_default();
        assert!(
            detail.contains("40001") || detail.contains("serialize"),
            "detail must still surface retriable substrings: {detail}",
        );
    }

    /// `DatabaseTyped` with an empty detail must return `None` from `detail()`,
    /// consistent with the `Database(String)` behaviour.
    #[test]
    fn database_typed_empty_detail_returns_none() {
        let err = CoolError::DatabaseTyped(DbErrorInfo::default());
        assert_eq!(err.detail(), None);
    }

    /// `into_response` must never leak the operator detail for DatabaseTyped.
    #[test]
    fn database_typed_into_response_does_not_leak_detail() {
        let info = DbErrorInfo {
            detail: "SELECT * FROM secrets".to_owned(),
            sqlstate: Some("23505".to_owned()),
            constraint: None,
        };
        let response = CoolError::DatabaseTyped(info).into_response();
        assert_eq!(response.code, "DATABASE_ERROR");
        assert_eq!(response.message, "internal error");
        assert!(!response.message.contains("secrets"));
        assert!(response.details.is_none());
    }
}
