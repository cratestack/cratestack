//! Map driver-level constraint errors back to the same per-field
//! `VALIDATION_ERROR` envelope the in-process validators produce.
//!
//! Phase 4 surfaces uniqueness, foreign-key, not-null, and length
//! breaches from the database as structured errors so the UI can put
//! the message next to the offending input instead of dropping a 500
//! on the user. We deliberately stay narrow: only well-known SQLSTATE
//! / SQLite extended codes are mapped; everything else falls through
//! as `DATABASE_ERROR`.

use cratestack_core::Model;
use cratestack_migrate::column_name;

use crate::validators::{FieldError, ValidationCode};

use super::DataError;

/// Inspect a sqlx error from a Postgres write. If it carries a
/// recognized SQLSTATE that maps to a field-level validation issue,
/// return `Some(DataError::Validation(...))`; otherwise `None` so the
/// caller forwards the original error untouched.
pub(crate) fn map_pg_error(model: Option<&Model>, error: &sqlx_core::Error) -> Option<DataError> {
    let db = match error {
        sqlx_core::Error::Database(db) => db,
        _ => return None,
    };
    let code = db.code()?;
    let message = db.message();

    let constraint = db.constraint();
    let column_hint = extract_quoted(message, "column");

    let field_name = column_hint
        .as_deref()
        .and_then(|col| model.and_then(|m| field_for_column(m, col)))
        .unwrap_or_else(|| column_hint.clone().unwrap_or_else(|| "*".to_owned()));

    let code_str = code.as_ref();
    let (vc, msg) = match code_str {
        // unique_violation
        "23505" => (
            ValidationCode::Unique,
            format!(
                "value violates unique constraint{}",
                constraint_suffix(constraint)
            ),
        ),
        // foreign_key_violation
        "23503" => (
            ValidationCode::ForeignKey,
            format!(
                "value violates foreign-key constraint{}",
                constraint_suffix(constraint)
            ),
        ),
        // not_null_violation
        "23502" => (
            ValidationCode::Required,
            "value must not be null".to_owned(),
        ),
        // string_data_right_truncation
        "22001" => (
            ValidationCode::Length,
            "value is too long for column".to_owned(),
        ),
        // invalid_text_representation
        "22P02" => (
            ValidationCode::TypeMismatch,
            "value is not valid for column type".to_owned(),
        ),
        // check_violation
        "23514" => (
            ValidationCode::Regex,
            format!(
                "value violates check constraint{}",
                constraint_suffix(constraint)
            ),
        ),
        _ => return None,
    };

    Some(DataError::Validation(vec![FieldError {
        field: field_name,
        code: vc,
        message: msg,
    }]))
}

/// Inspect a rusqlite error from a SQLite write. SQLite reports
/// constraint failures via `Error::SqliteFailure` with extended codes;
/// the column/table is in the message text.
pub(crate) fn map_sqlite_error(
    model: Option<&Model>,
    error: &rusqlite::Error,
) -> Option<DataError> {
    use rusqlite::ErrorCode;
    let (err, message) = match error {
        rusqlite::Error::SqliteFailure(err, Some(message)) => (err, message.as_str()),
        rusqlite::Error::SqliteFailure(err, None) => (err, ""),
        _ => return None,
    };

    if err.code != ErrorCode::ConstraintViolation {
        return None;
    }

    // SQLite messages look like:
    //   "UNIQUE constraint failed: posts.id"
    //   "NOT NULL constraint failed: posts.title"
    //   "FOREIGN KEY constraint failed"
    let column_hint = message.split_once(": ").and_then(|(_, rhs)| {
        rhs.split('.')
            .nth(1)
            .map(|c| c.trim_end_matches(';').trim().to_owned())
    });
    let field_name = column_hint
        .as_deref()
        .and_then(|col| model.and_then(|m| field_for_column(m, col)))
        .unwrap_or_else(|| column_hint.clone().unwrap_or_else(|| "*".to_owned()));

    // SQLite extended codes (see https://www.sqlite.org/rescode.html)
    const SQLITE_CONSTRAINT_UNIQUE: i32 = 2067;
    const SQLITE_CONSTRAINT_PRIMARYKEY: i32 = 1555;
    const SQLITE_CONSTRAINT_NOTNULL: i32 = 1299;
    const SQLITE_CONSTRAINT_FOREIGNKEY: i32 = 787;
    const SQLITE_CONSTRAINT_CHECK: i32 = 275;

    let extended = err.extended_code;
    let (code, msg) =
        if extended == SQLITE_CONSTRAINT_UNIQUE || extended == SQLITE_CONSTRAINT_PRIMARYKEY {
            (ValidationCode::Unique, "value violates unique constraint")
        } else if extended == SQLITE_CONSTRAINT_NOTNULL {
            (ValidationCode::Required, "value must not be null")
        } else if extended == SQLITE_CONSTRAINT_FOREIGNKEY {
            (
                ValidationCode::ForeignKey,
                "value violates foreign-key constraint",
            )
        } else if extended == SQLITE_CONSTRAINT_CHECK {
            (ValidationCode::Regex, "value violates check constraint")
        } else if message.to_ascii_uppercase().contains("UNIQUE") {
            (ValidationCode::Unique, "value violates unique constraint")
        } else if message.to_ascii_uppercase().contains("NOT NULL") {
            (ValidationCode::Required, "value must not be null")
        } else if message.to_ascii_uppercase().contains("FOREIGN KEY") {
            (
                ValidationCode::ForeignKey,
                "value violates foreign-key constraint",
            )
        } else {
            return None;
        };

    Some(DataError::Validation(vec![FieldError {
        field: field_name,
        code,
        message: msg.to_owned(),
    }]))
}

fn field_for_column(model: &Model, column: &str) -> Option<String> {
    model
        .fields
        .iter()
        .find(|f| column_name(&f.name) == column)
        .map(|f| f.name.clone())
}

fn constraint_suffix(constraint: Option<&str>) -> String {
    match constraint {
        Some(name) if !name.is_empty() => format!(" '{name}'"),
        _ => String::new(),
    }
}

/// Pull a `"thing"` literal that follows the given keyword from a
/// Postgres error message. Best-effort: returns `None` when the
/// message doesn't follow the expected shape.
fn extract_quoted(message: &str, keyword: &str) -> Option<String> {
    let needle = format!("{keyword} \"");
    let start = message.find(&needle)? + needle.len();
    let rest = &message[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_owned())
}

#[cfg(test)]
mod tests;
