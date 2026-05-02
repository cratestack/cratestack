use std::fmt::Write;
use std::marker::PhantomData;

use cratestack_core::{
    CoolError, CoolEventBus, CoolEventEnvelope, CoolEventFuture, ModelEventKind,
};
use cratestack_policy::ReadPolicy;

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
        .map_err(|error| CoolError::Database(error.to_string()))?;

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
                    .map_err(|error| CoolError::Database(error.to_string()))?;
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
                    .map_err(|db_error| CoolError::Database(db_error.to_string()))?;
                }
            }
        }

        Ok(delivered)
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub(crate) struct EventOutboxRow {
    pub(crate) event_id: uuid::Uuid,
    pub(crate) model: String,
    pub(crate) operation: String,
    pub(crate) occurred_at: chrono::DateTime<chrono::Utc>,
    pub(crate) payload: serde_json::Value,
    pub(crate) attempts: i64,
    pub(crate) last_error: Option<String>,
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
    .map_err(|error| CoolError::Database(error.to_string()))?;

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
    .map_err(|error| CoolError::Database(error.to_string()))?;

    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelColumn {
    pub rust_name: &'static str,
    pub sql_name: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct ModelDescriptor<M, PK> {
    pub schema_name: &'static str,
    pub table_name: &'static str,
    pub columns: &'static [ModelColumn],
    pub primary_key: &'static str,
    pub allowed_fields: &'static [&'static str],
    pub allowed_includes: &'static [&'static str],
    pub allowed_sorts: &'static [&'static str],
    pub read_allow_policies: &'static [ReadPolicy],
    pub read_deny_policies: &'static [ReadPolicy],
    pub detail_allow_policies: &'static [ReadPolicy],
    pub detail_deny_policies: &'static [ReadPolicy],
    pub create_allow_policies: &'static [ReadPolicy],
    pub create_deny_policies: &'static [ReadPolicy],
    pub update_allow_policies: &'static [ReadPolicy],
    pub update_deny_policies: &'static [ReadPolicy],
    pub delete_allow_policies: &'static [ReadPolicy],
    pub delete_deny_policies: &'static [ReadPolicy],
    pub create_defaults: &'static [CreateDefault],
    pub emitted_events: &'static [ModelEventKind],
    _marker: PhantomData<fn() -> (M, PK)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateDefaultType {
    Bool,
    Int,
    String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CreateDefault {
    pub column: &'static str,
    pub auth_field: &'static str,
    pub ty: CreateDefaultType,
    pub nullable: bool,
}

impl<M, PK> ModelDescriptor<M, PK> {
    pub const fn new(
        schema_name: &'static str,
        table_name: &'static str,
        columns: &'static [ModelColumn],
        primary_key: &'static str,
        allowed_fields: &'static [&'static str],
        allowed_includes: &'static [&'static str],
        allowed_sorts: &'static [&'static str],
        read_allow_policies: &'static [ReadPolicy],
        read_deny_policies: &'static [ReadPolicy],
        detail_allow_policies: &'static [ReadPolicy],
        detail_deny_policies: &'static [ReadPolicy],
        create_allow_policies: &'static [ReadPolicy],
        create_deny_policies: &'static [ReadPolicy],
        update_allow_policies: &'static [ReadPolicy],
        update_deny_policies: &'static [ReadPolicy],
        delete_allow_policies: &'static [ReadPolicy],
        delete_deny_policies: &'static [ReadPolicy],
        create_defaults: &'static [CreateDefault],
        emitted_events: &'static [ModelEventKind],
    ) -> Self {
        Self {
            schema_name,
            table_name,
            columns,
            primary_key,
            allowed_fields,
            allowed_includes,
            allowed_sorts,
            read_allow_policies,
            read_deny_policies,
            detail_allow_policies,
            detail_deny_policies,
            create_allow_policies,
            create_deny_policies,
            update_allow_policies,
            update_deny_policies,
            delete_allow_policies,
            delete_deny_policies,
            create_defaults,
            emitted_events,
            _marker: PhantomData,
        }
    }

    pub fn emits(&self, operation: ModelEventKind) -> bool {
        self.emitted_events.contains(&operation)
    }

    pub fn select_projection(&self) -> String {
        let mut sql = String::new();
        for (index, column) in self.columns.iter().enumerate() {
            if index > 0 {
                sql.push_str(", ");
            }
            let _ = write!(sql, "{} AS \"{}\"", column.sql_name, column.rust_name);
        }
        sql
    }
}
