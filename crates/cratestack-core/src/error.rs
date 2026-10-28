//! `CratestackError` — the framework's error type, its 4xx/5xx HTTP mapping,
//! and the public response envelope clients see on failure.
//!
//! 4xx variants carry caller-visible messages; 5xx variants keep the
//! operator detail off the wire and return a canned public message
//! while preserving the original string for `tracing` / `detail()`.

use std::borrow::Cow;

use http::StatusCode;
use serde::{Deserialize, Serialize};

mod aborted;
mod auth_refusal;
mod cuid_check;
mod response;
// Doctest-only: `AbortOwnership::Claimed` cannot be forged from outside.
#[cfg(doctest)]
mod aborted_doctests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_aborted;
#[cfg(test)]
mod tests_auth_refusal;

pub use aborted::{AbortOwnership, TransactionAbort};
pub use auth_refusal::{AuthRefusal, AuthRefusalKind, RESERVED_ENVELOPE_MEMBERS};
pub use cuid_check::parse_cuid;
pub use response::CratestackErrorResponse;
pub(crate) use response::visible_extra_members;

/// Structured information extracted from a driver-level database error.
///
/// Produced by `cratestack-sqlx`'s [`cratestack_error_from_sqlx`] when the
/// underlying `sqlx::Error` carries a typed `DatabaseError` (e.g.
/// `PgDatabaseError`). Consumers can inspect `constraint` and `code` without
/// substring-matching the stringified error message.
///
/// [`cratestack_error_from_sqlx`]: cratestack_sqlx::cratestack_error_from_sqlx
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
pub enum CratestackError {
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
    /// Conflict (409) with structured database information preserved from
    /// the driver — e.g. a unique-constraint violation. Prefer this over
    /// `Conflict(String)` when the conflict originates from a database
    /// error so [`CratestackError::db_sqlstate`] / [`CratestackError::db_constraint`]
    /// keep working for callers that inspect the typed fields regardless of
    /// whether the error surfaced as a 500 (`DatabaseTyped`) or a 409
    /// (`ConflictTyped`).
    #[error("conflict: {}", .0.detail)]
    ConflictTyped(DbErrorInfo),
    /// 409 — an `@isolation` transaction was rolled back because of
    /// concurrent updates (SQLSTATE `40001`/`40P01`, kept in
    /// `info.sqlstate`) and its retries ran out. Nothing that transaction
    /// wrote was committed, and the same request is expected to succeed when
    /// sent again — unlike [`Self::Conflict`], which a retry repeats.
    /// `info.detail` is the fixed public message; the driver's text is never
    /// carried. It is final: it is never classified as retriable itself, so
    /// an outer retry loop it propagates into does not run again because of
    /// it. An idempotency layer releases the key instead of recording it
    /// only when the dispatch that answered is the `@isolation` procedure
    /// whose own retries ran out ([`TransactionAbort::ownership`],
    /// [`Self::is_idempotency_replayable`]).
    #[error("transaction aborted: {}", .0.info.detail)]
    TransactionAborted(TransactionAbort),
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
    /// `cratestack_sqlx::cratestack_error_from_sqlx`.
    #[error("database: {0}")]
    Database(String),
    /// Database error with structured information preserved from the driver.
    ///
    /// Use [`CratestackError::db_sqlstate`] and [`CratestackError::db_constraint`] to
    /// access the typed fields without matching on this variant directly.
    #[error("database: {}", .0.detail)]
    DatabaseTyped(DbErrorInfo),
    #[error("internal: {0}")]
    Internal(String),
    /// 503 — the operation cannot proceed right now, but a retry (from
    /// scratch, not a resume) may succeed later. `String` is the
    /// public, safe-to-expose message, mirroring the other 4xx-style
    /// variants above.
    ///
    /// Introduced for `@@subscribe` SSE backpressure overflow
    /// (`docs/design/rpc-transport.md` §3.4/§3.4a: "bounded
    /// per-subscription send buffer; on overflow, emit
    /// `Error{code:"unavailable"}`") — the first caller of the RPC
    /// binding's already-reserved `"unavailable"` code.
    #[error("unavailable: {0}")]
    Unavailable(String),
    /// 429 — the caller exceeded a rate limit. `String` is the public,
    /// safe-to-expose message, same treatment as the 4xx variants above.
    ///
    /// Introduced for `cratestack-axum`'s `RateLimitLayer` (cratestack#846):
    /// the throttled response used to be a hand-built `text/plain` 429 with
    /// no error envelope at all, so a generated client decoded it as
    /// "unrecognized error body" instead of a typed code. Expressing the
    /// throttle as a `CratestackError` lets that response go through the
    /// exact same encode path every other error in the stack takes, rather
    /// than the layer inventing a second body shape.
    #[error("too many requests: {0}")]
    TooManyRequests(String),
    /// 401 or 403 — an [`AuthProvider`](crate::AuthProvider) refused the
    /// request and chose how the refusal reads: extra envelope members,
    /// `details` and `WWW-Authenticate` challenges (cratestack#1208). The
    /// code stays `UNAUTHORIZED` or `FORBIDDEN`. Built by
    /// [`AuthRefusal::into_error`].
    #[error("auth refused: {}", .0.message())]
    AuthRefused(AuthRefusal),
}

impl CratestackError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::BadRequest(_) => "BAD_REQUEST",
            Self::NotAcceptable(_) => "NOT_ACCEPTABLE",
            Self::Unauthorized(_) => "UNAUTHORIZED",
            Self::UnsupportedMediaType(_) => "UNSUPPORTED_MEDIA_TYPE",
            Self::Forbidden(_) => "FORBIDDEN",
            Self::NotFound(_) => "NOT_FOUND",
            Self::Conflict(_) | Self::ConflictTyped(_) => "CONFLICT",
            Self::TransactionAborted(_) => "TRANSACTION_ABORTED",
            Self::Validation(_) => "VALIDATION_ERROR",
            Self::PreconditionFailed(_) => "PRECONDITION_FAILED",
            Self::Codec(_) => "CODEC_ERROR",
            Self::Database(_) | Self::DatabaseTyped(_) => "DATABASE_ERROR",
            Self::Internal(_) => "INTERNAL_ERROR",
            Self::Unavailable(_) => "UNAVAILABLE",
            Self::TooManyRequests(_) => "TOO_MANY_REQUESTS",
            Self::AuthRefused(refusal) => match refusal.kind() {
                AuthRefusalKind::Unauthenticated => "UNAUTHORIZED",
                AuthRefusalKind::Forbidden => "FORBIDDEN",
            },
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
            Self::Conflict(_) | Self::ConflictTyped(_) | Self::TransactionAborted(_) => {
                StatusCode::CONFLICT
            }
            Self::Validation(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::PreconditionFailed(_) => StatusCode::PRECONDITION_FAILED,
            Self::Codec(_) => StatusCode::BAD_REQUEST,
            Self::Database(_) | Self::DatabaseTyped(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Unavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
            Self::TooManyRequests(_) => StatusCode::TOO_MANY_REQUESTS,
            Self::AuthRefused(refusal) => match refusal.kind() {
                AuthRefusalKind::Unauthenticated => StatusCode::UNAUTHORIZED,
                AuthRefusalKind::Forbidden => StatusCode::FORBIDDEN,
            },
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
            | Self::PreconditionFailed(s)
            | Self::Unavailable(s)
            | Self::TooManyRequests(s) => Cow::Borrowed(s.as_str()),
            Self::Codec(_) => Cow::Borrowed("invalid request payload"),
            Self::Database(_) | Self::DatabaseTyped(_) => Cow::Borrowed("internal error"),
            Self::Internal(_) => Cow::Borrowed("internal error"),
            // 4xx, like `Conflict(String)` — the driver's message is
            // caller-visible (matches the pre-existing behaviour of
            // `classify_unique_violation`, which built `Conflict` from
            // `db_err.message()`).
            Self::ConflictTyped(info) => Cow::Borrowed(info.detail.as_str()),
            Self::TransactionAborted(abort) => Cow::Borrowed(abort.info.detail.as_str()),
            Self::AuthRefused(refusal) => Cow::Borrowed(refusal.message()),
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
            | Self::Internal(s)
            | Self::Unavailable(s)
            | Self::TooManyRequests(s) => {
                if s.is_empty() {
                    None
                } else {
                    Some(s.as_str())
                }
            }
            Self::DatabaseTyped(info)
            | Self::ConflictTyped(info)
            | Self::TransactionAborted(TransactionAbort { info, .. }) => {
                if info.detail.is_empty() {
                    None
                } else {
                    Some(info.detail.as_str())
                }
            }
            Self::AuthRefused(refusal) => Some(refusal.message()).filter(|m| !m.is_empty()),
        }
    }

    /// Returns the SQLSTATE code if this is a `DatabaseTyped` error with a
    /// known code (e.g. `"23505"` for unique_violation).
    ///
    /// Always returns `None` for the legacy `Database(String)` variant; to
    /// get typed access, use `cratestack_sqlx::cratestack_error_from_sqlx` at the
    /// conversion site.
    pub fn db_sqlstate(&self) -> Option<&str> {
        match self {
            Self::DatabaseTyped(info)
            | Self::ConflictTyped(info)
            | Self::TransactionAborted(TransactionAbort { info, .. }) => info.sqlstate.as_deref(),
            _ => None,
        }
    }

    /// Returns the constraint name if this is a `DatabaseTyped` error that
    /// carries constraint information (e.g. `"accounts_email_key"`).
    ///
    /// Always returns `None` for the legacy `Database(String)` variant; to
    /// get typed access, use `cratestack_sqlx::cratestack_error_from_sqlx` at the
    /// conversion site.
    pub fn db_constraint(&self) -> Option<&str> {
        match self {
            Self::DatabaseTyped(info) | Self::ConflictTyped(info) => info.constraint.as_deref(),
            _ => None,
        }
    }

    /// The `WWW-Authenticate` header values the response carries: an
    /// [`AuthRefusal`]'s challenges, and none for every other error.
    pub fn auth_challenges(&self) -> &[String] {
        match self {
            Self::AuthRefused(refusal) => refusal.challenges(),
            _ => &[],
        }
    }

    pub fn into_response(self) -> CratestackErrorResponse {
        self.clone_envelope()
    }

    /// The envelope [`Self::into_response`] returns, without consuming
    /// the error (the RPC body is built from a borrowed error).
    pub(crate) fn clone_envelope(&self) -> CratestackErrorResponse {
        let code = self.code().to_owned();
        let message = self.public_message().into_owned();
        match self {
            Self::AuthRefused(refusal) => CratestackErrorResponse {
                code,
                message,
                details: refusal.details().cloned(),
                extra: refusal.members().clone(),
            },
            _ => CratestackErrorResponse {
                code,
                message,
                ..Default::default()
            },
        }
    }
}
