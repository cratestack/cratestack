//! `cratestack_event_outbox` table primitives: the row shape, its
//! hand-written `FromRow` (see the comment on the impl below for why
//! it isn't derived), table bootstrap, and the enqueue helper each
//! write path calls in-transaction. Split out of `descriptor.rs` to
//! keep that file under the project's ~200-line-per-file convention.

use cratestack_core::{CratestackError, CratestackEventEnvelope, ModelEventKind};

use crate::error::cool_error_from_sqlx;
use crate::sqlx;

#[derive(Debug, Clone)]
pub(crate) struct EventOutboxRow {
    pub(crate) event_id: uuid::Uuid,
    pub(crate) model: String,
    pub(crate) operation: String,
    pub(crate) occurred_at: chrono::DateTime<chrono::Utc>,
    pub(crate) payload: serde_json::Value,
    pub(crate) attempts: i64,
    pub(crate) last_error: Option<String>,
}

// Hand-written `FromRow` impl. We can't use `#[derive(sqlx::FromRow)]` because
// the derive macro hardcodes `::sqlx::*` paths that don't resolve through our
// `crate::sqlx` shim (the shim is module-scoped, not crate-aliased).
impl<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> for EventOutboxRow {
    fn from_row(row: &'r sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
        use sqlx::Row;
        Ok(Self {
            event_id: row.try_get("event_id")?,
            model: row.try_get("model")?,
            operation: row.try_get("operation")?,
            occurred_at: row.try_get("occurred_at")?,
            payload: row.try_get("payload")?,
            attempts: row.try_get("attempts")?,
            last_error: row.try_get("last_error")?,
        })
    }
}

impl EventOutboxRow {
    pub(crate) fn try_into_envelope(self) -> Result<CratestackEventEnvelope, CratestackError> {
        let _ = self.attempts;
        let _ = &self.last_error;
        Ok(CratestackEventEnvelope {
            event_id: self.event_id,
            model: self.model,
            operation: ModelEventKind::parse(&self.operation)?,
            occurred_at: self.occurred_at,
            data: self.payload,
        })
    }
}

/// Bootstraps `cratestack_event_outbox` if it doesn't already exist.
/// `pub` (rather than `pub(crate)`) since cratestack#507 ("option 3"):
/// `cratestack-studio`'s `[target.db]` write path calls this directly to
/// route Studio writes through the same outbox the generated server
/// uses, rather than duplicating the table DDL in a second crate where
/// it could drift out of sync with this one.
pub async fn ensure_event_outbox_table<'e, E>(executor: E) -> Result<(), CratestackError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS cratestack_event_outbox (\
            event_id UUID PRIMARY KEY, \
            model TEXT NOT NULL, \
            operation TEXT NOT NULL, \
            occurred_at TIMESTAMPTZ NOT NULL, \
            payload JSONB NOT NULL, \
            delivered_at TIMESTAMPTZ, \
            attempts BIGINT NOT NULL DEFAULT 0, \
            last_error TEXT\
        )",
    )
    .execute(executor)
    .await
    .map_err(cool_error_from_sqlx)?;

    Ok(())
}

/// Inserts one `cratestack_event_outbox` row. `pub` for the same reason
/// as [`ensure_event_outbox_table`] — see its doc comment.
///
/// `model` takes `&str` rather than the `&'static str` every in-crate
/// caller happens to pass (generated code's model names are always
/// `&'static str` literals): `cratestack-studio` parses `.cstack`
/// schemas at runtime, so its model names are owned `String`s with no
/// `'static` lifetime available. The function only ever borrows `model`
/// long enough to bind it as a query parameter, so relaxing the bound
/// costs nothing for the existing callers and is required for the new
/// one.
pub async fn enqueue_event_outbox<'e, E, T>(
    executor: E,
    model: &str,
    operation: ModelEventKind,
    data: &T,
) -> Result<(), CratestackError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    T: serde::Serialize,
{
    let payload = serde_json::to_value(data).map_err(|error| {
        CratestackError::Codec(format!("failed to encode event payload: {error}"))
    })?;
    sqlx::query(
        "INSERT INTO cratestack_event_outbox (event_id, model, operation, occurred_at, payload) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(model)
    .bind(operation.as_str())
    .bind(chrono::Utc::now())
    .bind(payload)
    .execute(executor)
    .await
    .map_err(cool_error_from_sqlx)?;

    Ok(())
}
