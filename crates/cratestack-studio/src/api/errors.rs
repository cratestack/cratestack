//! Shared error type for the Studio HTTP API. Maps internal failures
//! to JSON responses with stable shape:
//!
//! ```json
//! { "error": { "code": "UNKNOWN_TARGET", "message": "..." } }
//! ```

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

use crate::data::DataError;
use crate::validators::FieldError;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("unknown target '{0}'")]
    UnknownTarget(String),
    #[error("unknown model '{0}'")]
    UnknownModel(String),
    #[error("unknown field '{1}' on model '{0}'")]
    UnknownField(String, String),
    #[error("field '{1}' on model '{0}' is not a relation")]
    NotARelation(String, String),
    #[error("primary key '{0}' is not valid for this model: {1}")]
    InvalidPrimaryKey(String, String),
    #[error("model has no @id field; Studio v0 requires one")]
    NoPrimaryKey,
    #[error("operation not supported by this backend: {0}")]
    Unsupported(&'static str),
    #[error("target is read-only")]
    Forbidden,
    /// A `[target.db]` write to a model carrying `@version` and/or
    /// `@@emit(...)` was refused because the target hasn't opted into
    /// `allow_unsafe_writes` (cratestack#507). Direct SQL bypasses the
    /// descriptor path the generated server runs, so it neither bumps
    /// `@version` columns nor writes `cratestack_event_outbox` rows —
    /// silently, unless refused here.
    #[error(
        "target '{target}' would write model '{model}' straight to SQL, bypassing {annotations}; \
         set `allow_unsafe_writes = true` on this target's [target.db] to opt in"
    )]
    UnsafeDbWrite {
        target: String,
        model: String,
        annotations: String,
    },
    #[error("payload failed validation")]
    Validation(Vec<FieldError>),
    #[error("invalid request body: {0}")]
    BadRequest(String),
    #[error("database error: {0}")]
    Database(String),
    #[error("upstream API error: {0}")]
    Upstream(String),
    #[error("internal error: {0}")]
    Internal(String),
}

impl ApiError {
    fn status(&self) -> StatusCode {
        match self {
            ApiError::UnknownTarget(_) => StatusCode::NOT_FOUND,
            ApiError::UnknownModel(_) => StatusCode::NOT_FOUND,
            ApiError::UnknownField(_, _) => StatusCode::NOT_FOUND,
            ApiError::NotARelation(_, _) => StatusCode::BAD_REQUEST,
            ApiError::NoPrimaryKey => StatusCode::BAD_REQUEST,
            ApiError::InvalidPrimaryKey(_, _) => StatusCode::BAD_REQUEST,
            ApiError::BadRequest(_) => StatusCode::BAD_REQUEST,
            ApiError::Unsupported(_) => StatusCode::NOT_IMPLEMENTED,
            ApiError::Forbidden => StatusCode::FORBIDDEN,
            ApiError::UnsafeDbWrite { .. } => StatusCode::FORBIDDEN,
            ApiError::Validation(_) => StatusCode::UNPROCESSABLE_ENTITY,
            ApiError::Database(_) => StatusCode::INTERNAL_SERVER_ERROR,
            ApiError::Upstream(_) => StatusCode::BAD_GATEWAY,
            ApiError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn code(&self) -> &'static str {
        match self {
            ApiError::UnknownTarget(_) => "UNKNOWN_TARGET",
            ApiError::UnknownModel(_) => "UNKNOWN_MODEL",
            ApiError::UnknownField(_, _) => "UNKNOWN_FIELD",
            ApiError::NotARelation(_, _) => "NOT_A_RELATION",
            ApiError::NoPrimaryKey => "NO_PRIMARY_KEY",
            ApiError::InvalidPrimaryKey(_, _) => "INVALID_PRIMARY_KEY",
            ApiError::BadRequest(_) => "BAD_REQUEST",
            ApiError::Unsupported(_) => "UNSUPPORTED",
            ApiError::Forbidden => "FORBIDDEN",
            ApiError::UnsafeDbWrite { .. } => "UNSAFE_DB_WRITE",
            ApiError::Validation(_) => "VALIDATION_ERROR",
            ApiError::Database(_) => "DATABASE_ERROR",
            ApiError::Upstream(_) => "UPSTREAM_ERROR",
            ApiError::Internal(_) => "INTERNAL_ERROR",
        }
    }
}

impl From<DataError> for ApiError {
    fn from(err: DataError) -> Self {
        match err {
            DataError::UnknownModel { model } => ApiError::UnknownModel(model),
            DataError::UnknownField { model, field } => ApiError::UnknownField(model, field),
            DataError::NotARelation { model, field } => ApiError::NotARelation(model, field),
            DataError::NoPrimaryKey { .. } => ApiError::NoPrimaryKey,
            DataError::InvalidPrimaryKey { pk, reason, .. } => {
                ApiError::InvalidPrimaryKey(pk, reason)
            }
            DataError::Unsupported { what } => ApiError::Unsupported(what),
            DataError::Forbidden => ApiError::Forbidden,
            DataError::Validation(errors) => ApiError::Validation(errors),
            DataError::Db(e) => ApiError::Database(e.to_string()),
            DataError::EventOutbox(e) => ApiError::Database(e.to_string()),
            DataError::Sqlite(e) => ApiError::Database(e.to_string()),
            DataError::Api(e) => ApiError::Upstream(e.to_string()),
            DataError::BlockingJoin(msg) => ApiError::Internal(msg),
        }
    }
}

#[derive(Debug, Serialize)]
struct WireBody {
    error: WireError,
}

#[derive(Debug, Serialize)]
struct WireError {
    code: &'static str,
    message: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    fields: Vec<FieldError>,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let code = self.code();
        let status = self.status();
        let (message, fields) = match self {
            ApiError::Validation(errs) => ("payload failed validation".to_owned(), errs),
            other => (other.to_string(), Vec::new()),
        };
        let body = WireBody {
            error: WireError {
                code,
                message,
                fields,
            },
        };
        (status, Json(body)).into_response()
    }
}
