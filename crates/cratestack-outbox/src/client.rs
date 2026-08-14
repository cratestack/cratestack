//! [`OutboxClient`] — a thin, hand-written wrapper around the
//! `cratestack_outbox_events` table. See the crate-level docs for why this
//! goes straight to `sqlx` rather than through a generated cratestack
//! schema.

use chrono::{DateTime, Utc};
use cratestack_core::{CratestackError, TransactionIsolation};
use cratestack_sqlx::{cratestack_error_from_sqlx, run_in_isolated_tx_with_retries, sqlx};
use sqlx::Row;

use crate::drain::{DrainRequest, DrainResponse, HARD_MAX};
use crate::envelope::{EventEnvelope, NewEvent};

/// Service-local handle for emitting and draining outbox events. Cheap to
/// clone — it wraps only a `sqlx::PgPool`, which is itself `Clone` and
/// shares its underlying connection set.
#[derive(Clone)]
pub struct OutboxClient {
    pool: sqlx::PgPool,
}

impl OutboxClient {
    /// Build a client from a shared pool. Writes performed via
    /// [`OutboxClient::persist_in_tx`] commit atomically with the caller's
    /// business writes when both run against a transaction taken from this
    /// same pool.
    pub fn from_pool(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }

    /// The underlying pool, for a caller that wants to open its own
    /// transaction to pass to [`OutboxClient::persist_in_tx`].
    pub fn pool(&self) -> &sqlx::PgPool {
        &self.pool
    }

    /// Insert an event using a pool connection (not the caller's
    /// transaction). Returns the row id. Runs inside
    /// [`run_in_isolated_tx_with_retries`] so a transient serialization
    /// failure is retried rather than surfaced — single-row insert, so
    /// contention is unlikely, but the retry envelope costs nothing here
    /// and keeps this path consistent with [`OutboxClient::persist_in_tx`]'s
    /// transactional posture.
    pub async fn persist(&self, event: NewEvent) -> Result<String, CratestackError> {
        let row = build_insert_row(event, Utc::now());
        let id = row.id.clone();
        run_in_isolated_tx_with_retries(
            &self.pool,
            TransactionIsolation::ReadCommitted,
            1,
            |mut tx| {
                let row = row.clone();
                async move {
                    insert_event_row(&mut *tx, &row).await?;
                    Ok(((), tx))
                }
            },
        )
        .await?;
        Ok(id)
    }

    /// Insert an event inside the caller's own transaction. Use this when
    /// emitting alongside a business write so the two commit atomically —
    /// if the caller's transaction rolls back, the event row rolls back
    /// with it. This is the method the outbox pattern exists for; see the
    /// crate-level docs.
    pub async fn persist_in_tx<'tx>(
        &self,
        tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
        event: NewEvent,
    ) -> Result<String, CratestackError> {
        let row = build_insert_row(event, Utc::now());
        let id = row.id.clone();
        insert_event_row(&mut **tx, &row).await?;
        Ok(id)
    }

    /// Page through events in `id`-ascending order. UUIDv7's timestamp
    /// prefix means lexical sort order equals insertion order, so a plain
    /// `ORDER BY id ASC` cursor is enough — no separate sequence column.
    pub async fn drain(&self, req: &DrainRequest) -> Result<DrainResponse, CratestackError> {
        let limit = req.max.clamp(1, HARD_MAX);
        let rows: Vec<sqlx::postgres::PgRow> = match req.after_id.as_deref() {
            Some(cursor) => sqlx::query(
                "SELECT id, aggregate_type, aggregate_id, event_type, payload, occurred_at, correlation_id \
                 FROM cratestack_outbox_events \
                 WHERE id > $1 \
                 ORDER BY id ASC \
                 LIMIT $2",
            )
            .bind(cursor)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(cratestack_error_from_sqlx)?,
            None => sqlx::query(
                "SELECT id, aggregate_type, aggregate_id, event_type, payload, occurred_at, correlation_id \
                 FROM cratestack_outbox_events \
                 ORDER BY id ASC \
                 LIMIT $1",
            )
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(cratestack_error_from_sqlx)?,
        };

        let events: Vec<EventEnvelope> = rows
            .into_iter()
            .map(envelope_from_pg_row)
            .collect::<Result<Vec<_>, CratestackError>>()?;
        let next_cursor = events.last().map(|event| event.id.clone());
        Ok(DrainResponse {
            events,
            next_cursor,
        })
    }

    /// Delete events whose `occurred_at` is older than `cutoff`. Returns
    /// the number of rows removed.
    pub async fn gc_older_than(&self, cutoff: DateTime<Utc>) -> Result<u64, CratestackError> {
        let result = sqlx::query("DELETE FROM cratestack_outbox_events WHERE occurred_at < $1")
            .bind(cutoff)
            .execute(&self.pool)
            .await
            .map_err(cratestack_error_from_sqlx)?;
        Ok(result.rows_affected())
    }
}

#[derive(Clone)]
struct InsertRow {
    id: String,
    aggregate_type: String,
    aggregate_id: String,
    event_type: String,
    payload: serde_json::Value,
    occurred_at: DateTime<Utc>,
    correlation_id: Option<String>,
}

fn build_insert_row(event: NewEvent, now: DateTime<Utc>) -> InsertRow {
    InsertRow {
        id: uuid::Uuid::now_v7().to_string(),
        aggregate_type: event.aggregate_type,
        aggregate_id: event.aggregate_id,
        event_type: event.event_type,
        payload: event.payload,
        occurred_at: now,
        correlation_id: event.correlation_id,
    }
}

async fn insert_event_row<'c, E>(executor: E, row: &InsertRow) -> Result<(), CratestackError>
where
    E: sqlx::Executor<'c, Database = sqlx::Postgres>,
{
    sqlx::query(
        "INSERT INTO cratestack_outbox_events \
            (id, aggregate_type, aggregate_id, event_type, payload, occurred_at, correlation_id) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(&row.id)
    .bind(&row.aggregate_type)
    .bind(&row.aggregate_id)
    .bind(&row.event_type)
    .bind(sqlx::types::Json(&row.payload))
    .bind(row.occurred_at)
    .bind(row.correlation_id.as_deref())
    .execute(executor)
    .await
    .map_err(cratestack_error_from_sqlx)?;
    Ok(())
}

fn envelope_from_pg_row(row: sqlx::postgres::PgRow) -> Result<EventEnvelope, CratestackError> {
    let payload: sqlx::types::Json<serde_json::Value> =
        row.try_get("payload").map_err(cratestack_error_from_sqlx)?;
    Ok(EventEnvelope {
        id: row.try_get("id").map_err(cratestack_error_from_sqlx)?,
        aggregate_type: row
            .try_get("aggregate_type")
            .map_err(cratestack_error_from_sqlx)?,
        aggregate_id: row
            .try_get("aggregate_id")
            .map_err(cratestack_error_from_sqlx)?,
        event_type: row
            .try_get("event_type")
            .map_err(cratestack_error_from_sqlx)?,
        payload: payload.0,
        occurred_at: row
            .try_get("occurred_at")
            .map_err(cratestack_error_from_sqlx)?,
        correlation_id: row
            .try_get("correlation_id")
            .map_err(cratestack_error_from_sqlx)?,
    })
}
