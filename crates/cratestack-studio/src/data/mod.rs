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
pub(crate) mod db_errors;
pub(crate) mod model_info;
pub mod postgres;
pub mod relations;
pub mod sqlite;

pub(crate) use model_info::PkCast;
use serde::{Deserialize, Serialize};

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

/// Which operation the `/sql` endpoint should preview.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SqlOp {
    List,
    Get,
    Create,
    Update,
    Delete,
}

/// Rendered SQL preview returned by `/api/targets/:key/models/:m/sql`.
///
/// Parameters are listed by index in the order they're bound. The
/// shape is driver-agnostic — Postgres returns `$1`/`$2` placeholders,
/// SQLite returns `?1`/`?2`. The `notes` field carries any
/// driver-specific caveats so the UI can show them without parsing the
/// SQL text.
#[derive(Debug, Clone, Serialize)]
pub struct SqlPreview {
    pub driver: &'static str,
    pub sql: String,
    pub params: Vec<SqlParam>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SqlParam {
    pub index: u32,
    pub binding: String,
    pub kind: &'static str,
}

/// One physical column observed in the live database. Used by the
/// drift endpoint to compare schema-declared shape against what the
/// driver actually sees.
#[derive(Debug, Clone, Serialize)]
pub struct ColumnSnapshot {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum DataError {
    #[error("unknown model '{model}' in target")]
    UnknownModel { model: String },
    #[error("model '{model}' has no field '{field}'")]
    UnknownField { model: String, field: String },
    #[error("field '{field}' on model '{model}' is not a relation")]
    NotARelation { model: String, field: String },
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
    /// Target is in read-only mode (`mode = "ro"`).
    #[error("target is read-only")]
    Forbidden,
    /// One or more field-level validators rejected the payload. The
    /// per-field detail is forwarded to the API envelope as
    /// `VALIDATION_ERROR`.
    #[error("payload failed validation")]
    Validation(Vec<crate::validators::FieldError>),
    #[error("database error: {0}")]
    Db(#[from] sqlx_core::Error),
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("upstream API error: {0}")]
    Api(#[from] reqwest::Error),
    #[error("blocking task panicked: {0}")]
    BlockingJoin(String),
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

    /// Paginated rows of `target_model` whose `filter_column` equals
    /// `filter_value`. Powers the relation-follow endpoint: the
    /// caller resolves the relation to a (model, column, cast, value)
    /// tuple via [`relations::resolve_relation`], and the source runs
    /// the SQL.
    async fn follow(
        &self,
        target_model: &str,
        filter_column: &str,
        filter_cast: PkCast,
        filter_value: &str,
        page: PageRequest<'_>,
    ) -> Result<Page, DataError>;

    /// INSERT a row into `model` using the (validated) payload. The
    /// returned `Row` is the persisted row as the database stores it
    /// (so generated defaults like `@default(dbgenerated())` are
    /// visible).
    async fn create(
        &self,
        model: &str,
        payload: &Row,
    ) -> Result<Row, DataError>;

    /// UPDATE the row identified by `pk` with the (validated, partial)
    /// payload. Returns the updated row. `Ok(None)` if no row
    /// matched.
    async fn update(
        &self,
        model: &str,
        pk: &str,
        payload: &Row,
    ) -> Result<Option<Row>, DataError>;

    /// DELETE the row identified by `pk`. Returns the deleted row, or
    /// `Ok(None)` if no row matched.
    async fn delete(
        &self,
        model: &str,
        pk: &str,
    ) -> Result<Option<Row>, DataError>;

    /// Render the SQL Studio would run for `op` on `model` without
    /// touching the database. `pk` is required for GET / UPDATE /
    /// DELETE; `payload` is optional for CREATE / UPDATE (when
    /// provided, the bound parameters reflect its column order). The
    /// returned struct carries the rendered SQL plus a parameter list.
    ///
    /// Backends that don't speak SQL (the deployed-API source) return
    /// `DataError::Unsupported`.
    async fn preview_sql(
        &self,
        op: SqlOp,
        model: &str,
        pk: Option<&str>,
        payload: Option<&Row>,
    ) -> Result<SqlPreview, DataError>;

    /// Snapshot the live database columns for `model`'s table. Used by
    /// the drift endpoint to compare against schema-declared columns.
    /// `Ok(None)` when the table doesn't exist in the live database.
    /// Backends without a database (API source) return
    /// `DataError::Unsupported`.
    async fn inspect_columns(
        &self,
        model: &str,
    ) -> Result<Option<Vec<ColumnSnapshot>>, DataError>;
}
