//! The [`DataSource`] trait — Studio's read/write/preview/inspect
//! abstraction over a backend.
//!
//! Each [`crate::workspace::LoadedTarget`] owns one `Arc<dyn DataSource>`;
//! request handlers delegate to it without knowing whether the target
//! is a Postgres pool, a SQLite connection, or a reqwest client.

use super::model_info::PkCast;
use super::{ColumnSnapshot, DataError, Page, PageRequest, Row, SqlOp, SqlPreview};

/// Backend interface used by every Studio request handler. Each
/// `LoadedTarget` holds an `Arc<dyn DataSource>` and delegates to it.
#[async_trait::async_trait]
pub trait DataSource: Send + Sync + std::fmt::Debug {
    /// List paginated rows of one model. Order is by ascending primary
    /// key so cursors are stable.
    async fn list(&self, model: &str, page: PageRequest<'_>) -> Result<Page, DataError>;

    /// Fetch one row by its primary key value. Returns `Ok(None)` if
    /// the row doesn't exist.
    async fn get(&self, model: &str, pk: &str) -> Result<Option<Row>, DataError>;

    /// Paginated rows of `target_model` whose `filter_column` equals
    /// `filter_value`. Powers the relation-follow endpoint: the caller
    /// resolves the relation to a (model, column, cast, value) tuple
    /// via [`super::relations::resolve_relation`], and the source runs
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
    async fn create(&self, model: &str, payload: &Row) -> Result<Row, DataError>;

    /// UPDATE the row identified by `pk` with the (validated, partial)
    /// payload. Returns the updated row. `Ok(None)` if no row matched.
    async fn update(&self, model: &str, pk: &str, payload: &Row) -> Result<Option<Row>, DataError>;

    /// DELETE the row identified by `pk`. Returns the deleted row, or
    /// `Ok(None)` if no row matched.
    async fn delete(&self, model: &str, pk: &str) -> Result<Option<Row>, DataError>;

    /// Render the SQL Studio would run for `op` on `model` without
    /// touching the database. `pk` is required for GET / UPDATE /
    /// DELETE; `payload` is optional for CREATE / UPDATE (when
    /// provided, the bound parameters reflect its column order).
    ///
    /// Backends that don't speak SQL (the deployed-API source) return
    /// [`DataError::Unsupported`].
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
    /// [`DataError::Unsupported`].
    async fn inspect_columns(&self, model: &str) -> Result<Option<Vec<ColumnSnapshot>>, DataError>;
}
