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

#[derive(Debug, thiserror::Error)]
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
    #[error("database: {0}")]
    Database(String),
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
            Self::Database(_) => "DATABASE_ERROR",
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
            Self::Database(_) => StatusCode::INTERNAL_SERVER_ERROR,
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
            Self::Database(_) => Cow::Borrowed("internal error"),
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
            "invalid cuid '{}': expected a lowercase alphanumeric id starting with 'c'",
            value,
        )))
    }
}

fn is_valid_cuid(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if first != 'c' || value.len() < 2 {
        return false;
    }
    chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit())
}
