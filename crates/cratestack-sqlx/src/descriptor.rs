use crate::sqlx;

use cratestack_core::{
    CoolError, CoolEventBus, CoolEventEnvelope, CoolEventFuture, ModelEventKind,
};

use crate::error::cool_error_from_sqlx;

#[derive(Debug, Clone)]
pub struct SqlxRuntime {
    pool: sqlx::PgPool,
    events: CoolEventBus,
}

impl SqlxRuntime {
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self {
            pool,
            events: CoolEventBus::default(),
        }
    }

    pub fn pool(&self) -> &sqlx::PgPool {
        &self.pool
    }

    #[doc(hidden)]
    pub fn subscribe<F>(&self, model: &'static str, operation: ModelEventKind, handler: F)
    where
        F: Fn(CoolEventEnvelope) -> CoolEventFuture + Send + Sync + 'static,
    {
        self.events.subscribe(model, operation, handler);
    }

    #[doc(hidden)]
    pub async fn drain_event_outbox(&self) -> Result<usize, CoolError> {
        ensure_event_outbox_table(&self.pool).await?;

        let rows = sqlx::query_as::<_, EventOutboxRow>(
            "SELECT event_id, model, operation, occurred_at, payload, attempts, last_error \
             FROM cratestack_event_outbox \
             WHERE delivered_at IS NULL \
             ORDER BY occurred_at ASC, event_id ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(cool_error_from_sqlx)?;

        let mut delivered = 0usize;
        for row in rows {
            let event_id = row.event_id;
            let envelope = row.try_into_envelope()?;
            match self.events.emit(envelope).await {
                Ok(()) => {
                    sqlx::query(
                        "UPDATE cratestack_event_outbox \
                         SET delivered_at = NOW(), last_error = NULL, attempts = attempts + 1 \
                         WHERE event_id = $1",
                    )
                    .bind(event_id)
                    .execute(&self.pool)
                    .await
                    .map_err(cool_error_from_sqlx)?;
                    delivered += 1;
                }
                Err(error) => {
                    sqlx::query(
                        "UPDATE cratestack_event_outbox \
                         SET attempts = attempts + 1, last_error = $2 \
                         WHERE event_id = $1",
                    )
                    .bind(event_id)
                    .bind(error.to_string())
                    .execute(&self.pool)
                    .await
                    .map_err(cool_error_from_sqlx)?;
                }
            }
        }

        Ok(delivered)
    }
}

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
    pub(crate) fn try_into_envelope(self) -> Result<CoolEventEnvelope, CoolError> {
        let _ = self.attempts;
        let _ = &self.last_error;
        Ok(CoolEventEnvelope {
            event_id: self.event_id,
            model: self.model,
            operation: ModelEventKind::parse(&self.operation)?,
            occurred_at: self.occurred_at,
            data: self.payload,
        })
    }
}

pub(crate) async fn ensure_event_outbox_table<'e, E>(executor: E) -> Result<(), CoolError>
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

pub(crate) async fn enqueue_event_outbox<'e, E, T>(
    executor: E,
    model: &'static str,
    operation: ModelEventKind,
    data: &T,
) -> Result<(), CoolError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    T: serde::Serialize,
{
    let payload = serde_json::to_value(data)
        .map_err(|error| CoolError::Codec(format!("failed to encode event payload: {error}")))?;
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
