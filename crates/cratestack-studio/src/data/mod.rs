//! Abstraction over Studio's data backends.
//!
//! Two implementations land in Phase 1:
//! - [`postgres::PostgresSource`] — a sqlx Postgres pool. Phase 1a.
//! - [`api::ApiSource`] — a reqwest client against a deployed cratestack
//!   service. Schema-fetch is implemented in Phase 1a; record reads
//!   land in a later phase.
//!
//! A future SQLite source will use `rusqlite` directly (sqlx-sqlite
//! conflicts with the workspace's existing rusqlite pin on
//! `libsqlite3-sys`).

pub mod api;
pub mod postgres;

use serde::Serialize;

/// One database row, projected as a JSON object so the API layer can
/// pass it through without knowing per-column types. Keys are field
/// names exactly as declared in the `.cstack` model — column-name
/// snake_casing is reversed before serialization where applicable.
pub type Row = serde_json::Map<String, serde_json::Value>;

/// Page of rows returned from [`DataSource::list`] /
/// [`DataSource::follow`]. The cursor is opaque to the client and is
/// only meaningful when passed back as `cursor=` on the next request.
#[derive(Debug, Clone, Serialize)]
pub struct Page {
    pub rows: Vec<Row>,
    pub next_cursor: Option<String>,
}

/// Request shape for paginated endpoints. `limit` is clamped to a
/// per-target maximum inside each source impl.
#[derive(Debug, Clone, Copy, Default)]
pub struct PageRequest<'a> {
    pub cursor: Option<&'a str>,
    pub limit: Option<u32>,
}

/// Default page size when the client doesn't pass `limit=`.
pub const DEFAULT_PAGE_LIMIT: u32 = 50;
/// Hard cap regardless of what the client asks for.
pub const MAX_PAGE_LIMIT: u32 = 500;

#[derive(Debug, thiserror::Error)]
pub enum DataError {
    #[error("unknown model '{model}' in target")]
    UnknownModel { model: String },
    #[error("model '{model}' has no @id field; Studio v0 requires one")]
    NoPrimaryKey { model: String },
    #[error("primary key value '{pk}' is not valid for model '{model}': {reason}")]
    InvalidPrimaryKey {
        model: String,
        pk: String,
        reason: String,
    },
    #[error("operation not supported by this backend: {what}")]
    Unsupported { what: &'static str },
    #[error("database error: {0}")]
    Db(#[from] sqlx_core::Error),
    #[error("upstream API error: {0}")]
    Api(#[from] reqwest::Error),
}

/// Backend interface used by the read endpoints. Each `LoadedTarget`
/// owns one `Arc<dyn DataSource>`.
#[async_trait::async_trait]
pub trait DataSource: Send + Sync + std::fmt::Debug {
    /// List paginated rows of one model. Order is by ascending primary
    /// key so cursors are stable.
    async fn list(&self, model: &str, page: PageRequest<'_>) -> Result<Page, DataError>;

    /// Fetch one row by its primary key value. Returns `Ok(None)` if
    /// the row doesn't exist.
    async fn get(&self, model: &str, pk: &str) -> Result<Option<Row>, DataError>;
}
