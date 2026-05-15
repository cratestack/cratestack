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
            ApiError::Unsupported(_) => StatusCode::NOT_IMPLEMENTED,
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
            ApiError::Unsupported(_) => "UNSUPPORTED",
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
            DataError::Db(e) => ApiError::Database(e.to_string()),
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
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = WireBody {
            error: WireError {
                code: self.code(),
                message: self.to_string(),
            },
        };
        (self.status(), Json(body)).into_response()
    }
}
