//! Write-error classifier shared by the batch create/update/upsert paths
//! and the single-row create/update paths.
//!
//! The batch paths run their terminal INSERT/UPDATE/UPSERT inside a
//! per-item SAVEPOINT; the single-row paths run it as the whole
//! statement. Both need a unique-constraint violation (SQLSTATE 23505)
//! to surface as a 409 rather than the 500 that `cool_error_from_sqlx`
//! alone would produce. The sqlstate/constraint captured from the driver
//! is preserved via [`CratestackError::ConflictTyped`] so
//! `CratestackError::db_sqlstate`/`db_constraint` keep working regardless of
//! whether the violation was classified as a conflict or fell through to
//! [`cool_error_from_sqlx`]'s `DatabaseTyped`.

use cratestack_core::{CratestackError, DbErrorInfo};

use crate::{cool_error_from_sqlx, sqlx};

pub(crate) fn classify_unique_violation(error: sqlx::Error) -> CratestackError {
    if let sqlx::Error::Database(db_err) = &error
        && let Some(code) = db_err.code()
        && code == "23505"
    {
        return CratestackError::ConflictTyped(DbErrorInfo {
            detail: db_err.message().to_owned(),
            sqlstate: Some(code.into_owned()),
            constraint: db_err.constraint().map(ToOwned::to_owned),
        });
    }
    cool_error_from_sqlx(error)
}
