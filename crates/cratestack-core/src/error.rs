//! `CoolError` — the framework's error type, its 4xx/5xx HTTP mapping,
//! and the public response envelope clients see on failure.
//!
//! 4xx variants carry caller-visible messages; 5xx variants keep the
//! operator detail off the wire and return a canned public message
//! while preserving the original string for `tracing` / `detail()`.

use std::borrow::Cow;

use http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::value::Value;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoolErrorResponse {
    pub code: String,
    pub message: String,
    pub details: Option<Value>,
}

/// Structured information extracted from a driver-level database error.
///
/// Produced by `cratestack-sqlx`'s [`cool_error_from_sqlx`] when the
/// underlying `sqlx::Error` carries a typed `DatabaseError` (e.g.
/// `PgDatabaseError`). Consumers can inspect `constraint` and `code` without
/// substring-matching the stringified error message.
///
/// [`cool_error_from_sqlx`]: cratestack_sqlx::cool_error_from_sqlx
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct DbErrorInfo {
    /// The operator-visible detail string (equivalent to `error.to_string()`).
    pub detail: String,
    /// The five-character SQLSTATE code (`"23505"` for unique_violation, etc.).
    /// `None` when the driver did not surface a code.
    pub sqlstate: Option<String>,
    /// The constraint name reported by the database (`"accounts_email_key"`,
    /// etc.). `None` when the error is not constraint-related.
    pub constraint: Option<String>,
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CoolError {
    /// 4xx — `String` is the public message returned to the client.
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("not acceptable: {0}")]
    NotAcceptable(String),
    #[error("unauthorized: {0}")]
    Unauthorized(String),
    #[error("unsupported media type: {0}")]
    UnsupportedMediaType(String),
    #[error("forbidden: {0}")]
    Forbidden(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("validation: {0}")]
    Validation(String),
    #[error("precondition failed: {0}")]
    PreconditionFailed(String),
    /// 5xx — `String` is operator-only detail. Never returned to clients;
    /// the public message is a fixed canned string per variant.
    #[error("codec: {0}")]
    Codec(String),
    /// Database error with only a stringified detail. Preserved for
    /// back-compat; new code should prefer `DatabaseTyped` produced by
    /// `cratestack_sqlx::cool_error_from_sqlx`.
    #[error("database: {0}")]
    Database(String),
    /// Database error with structured information preserved from the driver.
    ///
    /// Use [`CoolError::db_sqlstate`] and [`CoolError::db_constraint`] to
    /// access the typed fields without matching on this variant directly.
    #[error("database: {}", .0.detail)]
    DatabaseTyped(DbErrorInfo),
    #[error("internal: {0}")]
    Internal(String),
}

impl CoolError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::BadRequest(_) => "BAD_REQUEST",
            Self::NotAcceptable(_) => "NOT_ACCEPTABLE",
            Self::Unauthorized(_) => "UNAUTHORIZED",
            Self::UnsupportedMediaType(_) => "UNSUPPORTED_MEDIA_TYPE",
            Self::Forbidden(_) => "FORBIDDEN",
            Self::NotFound(_) => "NOT_FOUND",
            Self::Conflict(_) => "CONFLICT",
            Self::Validation(_) => "VALIDATION_ERROR",
            Self::PreconditionFailed(_) => "PRECONDITION_FAILED",
            Self::Codec(_) => "CODEC_ERROR",
            Self::Database(_) | Self::DatabaseTyped(_) => "DATABASE_ERROR",
            Self::Internal(_) => "INTERNAL_ERROR",
        }
    }

    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::NotAcceptable(_) => StatusCode::NOT_ACCEPTABLE,
            Self::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            Self::UnsupportedMediaType(_) => StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Self::Forbidden(_) => StatusCode::FORBIDDEN,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::Validation(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::PreconditionFailed(_) => StatusCode::PRECONDITION_FAILED,
            Self::Codec(_) => StatusCode::BAD_REQUEST,
            Self::Database(_) | Self::DatabaseTyped(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// Public, safe-to-expose message returned in HTTP responses.
    ///
    /// For 4xx variants this is the caller-supplied string. For 5xx variants
    /// this is a fixed canned message; the caller-supplied string flows to
    /// `detail` instead and is recorded via tracing only.
    pub fn public_message(&self) -> Cow<'_, str> {
        match self {
            Self::BadRequest(s)
            | Self::NotAcceptable(s)
            | Self::Unauthorized(s)
            | Self::UnsupportedMediaType(s)
            | Self::Forbidden(s)
            | Self::NotFound(s)
            | Self::Conflict(s)
            | Self::Validation(s)
            | Self::PreconditionFailed(s) => Cow::Borrowed(s.as_str()),
            Self::Codec(_) => Cow::Borrowed("invalid request payload"),
            Self::Database(_) | Self::DatabaseTyped(_) => Cow::Borrowed("internal error"),
            Self::Internal(_) => Cow::Borrowed("internal error"),
        }
    }

    /// Operator-only detail string. For 5xx variants this is the message
    /// supplied at construction time; for 4xx variants this returns the same
    /// string as `public_message` (callers are expected to pre-redact 4xx
    /// messages they emit).
    pub fn detail(&self) -> Option<&str> {
        match self {
            Self::BadRequest(s)
            | Self::NotAcceptable(s)
            | Self::Unauthorized(s)
            | Self::UnsupportedMediaType(s)
            | Self::Forbidden(s)
            | Self::NotFound(s)
            | Self::Conflict(s)
            | Self::Validation(s)
            | Self::PreconditionFailed(s)
            | Self::Codec(s)
            | Self::Database(s)
            | Self::Internal(s) => {
                if s.is_empty() {
                    None
                } else {
                    Some(s.as_str())
                }
            }
            Self::DatabaseTyped(info) => {
                if info.detail.is_empty() {
                    None
                } else {
                    Some(info.detail.as_str())
                }
            }
        }
    }

    /// Returns the SQLSTATE code if this is a `DatabaseTyped` error with a
    /// known code (e.g. `"23505"` for unique_violation).
    ///
    /// Always returns `None` for the legacy `Database(String)` variant; to
    /// get typed access, use `cratestack_sqlx::cool_error_from_sqlx` at the
    /// conversion site.
    pub fn db_sqlstate(&self) -> Option<&str> {
        match self {
            Self::DatabaseTyped(info) => info.sqlstate.as_deref(),
            _ => None,
        }
    }

    /// Returns the constraint name if this is a `DatabaseTyped` error that
    /// carries constraint information (e.g. `"accounts_email_key"`).
    ///
    /// Always returns `None` for the legacy `Database(String)` variant; to
    /// get typed access, use `cratestack_sqlx::cool_error_from_sqlx` at the
    /// conversion site.
    pub fn db_constraint(&self) -> Option<&str> {
        match self {
            Self::DatabaseTyped(info) => info.constraint.as_deref(),
            _ => None,
        }
    }

    pub fn into_response(self) -> CoolErrorResponse {
        let code = self.code().to_owned();
        let message = self.public_message().into_owned();
        CoolErrorResponse {
            code,
            message,
            details: None,
        }
    }
}

pub fn parse_cuid(value: &str) -> Result<String, CoolError> {
    if is_valid_cuid(value) {
        Ok(value.to_owned())
    } else {
        Err(CoolError::BadRequest(format!(
            "invalid cuid '{}': expected a lowercase alphanumeric id (2-32 chars)",
            value,
        )))
    }
}

/// Minimum accepted length for a `Cuid` scalar value.
///
/// cuid v1 ids are at least 2 characters (the `'c'` prefix plus at least one
/// more character); cuid2 ids can be as short as 2 characters too, so this
/// bound covers both formats.
const CUID_MIN_LEN: usize = 2;

/// Maximum accepted length for a `Cuid` scalar value.
///
/// cuid2 defaults to 24 characters but its length is configurable by the
/// generator; 32 gives generous headroom above the default while still
/// rejecting pathological/oversized input.
const CUID_MAX_LEN: usize = 32;

/// Validates that `value` is a plausible cuid, accepting both the legacy
/// cuid v1 shape (`'c'`-prefixed) and the current cuid2 shape (no fixed
/// prefix; the first character is a uniform random lowercase letter).
///
/// This is intentionally a format guard, not a full cuid2
/// checksum/fingerprint verification: lowercase alphanumeric only,
/// non-empty, bounded length.
fn is_valid_cuid(value: &str) -> bool {
    if !(CUID_MIN_LEN..=CUID_MAX_LEN).contains(&value.len()) {
        return false;
    }
    value
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit())
}
