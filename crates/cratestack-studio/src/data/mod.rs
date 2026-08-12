//! Abstraction over Studio's data backends.
//!
//! Three implementations land in Phase 1:
//! - [`postgres::PostgresSource`] — a sqlx Postgres pool. Phase 1a.
//! - [`sqlite::SqliteSource`] — rusqlite-backed for local databases.
//! - [`api::ApiSource`] — a reqwest client against a deployed cratestack
//!   service.

pub mod api;
pub(crate) mod common;
pub(crate) mod db_errors;
pub(crate) mod model_info;
pub mod postgres;
pub mod relations;
pub mod source;
pub mod sqlite;

pub(crate) use model_info::PkCast;
use serde::{Deserialize, Serialize};

pub use source::DataSource;

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

/// Outcome of a **plan-only** EXPLAIN.
///
/// Exactly one side is populated: `text` when the driver produced a
/// plan, `note` when it declined to. "Declined" is a first-class
/// answer here rather than an error, because the caller still wants the
/// rendered SQL next to the explanation of why there's no plan
/// beside it.
#[derive(Debug, Clone)]
pub struct QueryPlan {
    pub text: Option<String>,
    pub note: Option<String>,
}

impl QueryPlan {
    pub(crate) fn text(plan: String) -> Self {
        Self {
            text: Some(plan),
            note: None,
        }
    }

    pub(crate) fn note(reason: impl Into<String>) -> Self {
        Self {
            text: None,
            note: Some(reason.into()),
        }
    }
}

/// Why Studio refuses to plan a mutation.
///
/// Plain `EXPLAIN` (Postgres) and `EXPLAIN QUERY PLAN` (SQLite) only
/// *plan* a statement — neither executes it, so explaining an INSERT
/// would in fact be harmless. `EXPLAIN ANALYZE` is the one that runs
/// the statement for real, and Studio never emits it: there is no
/// config knob, no query parameter, and no code path that can produce
/// it.
///
/// The read-only restriction is therefore belt-and-braces rather than
/// strictly necessary, and it is kept for two reasons. First, it means
/// no future edit can turn "we planned your DELETE" into "we ran your
/// DELETE" by adding one keyword — the mutation never reaches a driver
/// at all. Second, it is honest about what we could show anyway: the
/// CREATE/UPDATE previews bind placeholder sample values (`…`) for
/// columns whose real types are numeric, boolean or jsonb, so asking
/// Postgres to plan them would fail on type inference rather than
/// produce a plan worth reading.
pub(crate) const EXPLAIN_READ_ONLY_NOTE: &str = concat!(
    "EXPLAIN is limited to read operations (list, get). ",
    "Studio never runs EXPLAIN ANALYZE, so a mutation is never planned or executed here.",
);

/// Get needs a concrete key to plan against.
pub(crate) const EXPLAIN_NEEDS_PK_NOTE: &str =
    "EXPLAIN for `get` needs a `pk=` value to bind — select a row first.";

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
    /// A `create_routed`/`update_routed`/`delete_routed` call
    /// (cratestack#507) failed while bootstrapping or writing
    /// `cratestack_event_outbox` via `cratestack_sqlx`'s shared
    /// primitives.
    #[error("event outbox write failed: {0}")]
    EventOutbox(#[from] cratestack_core::CoolError),
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("upstream API error: {0}")]
    Api(#[from] reqwest::Error),
    #[error("blocking task panicked: {0}")]
    BlockingJoin(String),
}
