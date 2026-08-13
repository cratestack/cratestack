//! The **routed** write path — cratestack#507's "option 3": applies
//! `@version` bumping and, for a model's `@@emit(...)`-declared
//! operations, a `cratestack_event_outbox` row, using the exact table
//! primitives ([`cratestack_sqlx::enqueue_event_outbox`],
//! [`cratestack_sqlx::ensure_event_outbox_table`]) the generated
//! server's own descriptor path (`cratestack-sqlx::query::write::*`)
//! writes through — not a reimplementation of that table's schema.
//!
//! Reached only when
//! [`crate::api::records::guards::require_write_mode`] decides the
//! write is `Routed`: every annotation the model declares can be
//! honored for real on this backend. [`super::ops`] carries the
//! unrouted equivalents (used for a model with no relevant annotations,
//! and for the legacy `allow_unsafe_writes` bypass on an unroutable
//! `@@emit`).
//!
//! `@version` bumping needs no transaction — it's one more assignment in
//! the same `UPDATE`/`INSERT` statement. The event-outbox row does: it
//! must land in the same transaction as the row mutation, or a crash
//! between the two could commit one without the other.

use cratestack_core::{ModelEventKind, Schema};

use crate::data::model_info::{emitted_events, resolve_model, version_column};
use crate::data::{DataError, Row};

use super::bindings::{TypedValue, collect_payload};
use super::exec::{delete_returning, insert_returning, update_returning};
use super::sql::{build_delete_sql, build_insert_sql, build_update_sql};

pub(super) async fn create_routed(
    schema: &Schema,
    pool: &sqlx_postgres::PgPool,
    model: &str,
    payload: &Row,
) -> Result<Row, DataError> {
    let (resolved, info) = resolve_model(schema, model)?;
    let version_col = version_column(resolved);
    let (mut cols, mut binds) =
        collect_payload(schema, model, &info, payload, version_col.as_deref());
    // Seed the optimistic-lock column server-side, exactly like
    // `cratestack-sqlx::query::write::create_exec`'s "seed the
    // optimistic-lock column server-side" comment — `collect_payload`
    // above already stripped any client-supplied `version` key, so this
    // is never a duplicate column.
    if let Some(v) = &version_col {
        cols.push(v.clone());
        binds.push(TypedValue::Int(0));
    }
    let sql = build_insert_sql(&info, &cols);

    if !emitted_events(resolved).contains(&ModelEventKind::Created) {
        return insert_returning(pool, &sql, &binds, resolved).await;
    }
    let mut tx = pool.begin().await?;
    cratestack_sqlx::ensure_event_outbox_table(&mut *tx).await?;
    let row = insert_returning(&mut *tx, &sql, &binds, resolved).await?;
    cratestack_sqlx::enqueue_event_outbox(&mut *tx, model, ModelEventKind::Created, &row).await?;
    tx.commit().await?;
    Ok(row)
}

pub(super) async fn update_routed(
    schema: &Schema,
    pool: &sqlx_postgres::PgPool,
    model: &str,
    pk: &str,
    payload: &Row,
) -> Result<Option<Row>, DataError> {
    let (resolved, info) = resolve_model(schema, model)?;
    let version_col = version_column(resolved);
    let (cols, binds) = collect_payload(schema, model, &info, payload, version_col.as_deref());
    let sql = build_update_sql(&info, &cols, version_col.as_deref());

    if !emitted_events(resolved).contains(&ModelEventKind::Updated) {
        return update_returning(pool, &sql, &binds, pk, resolved).await;
    }
    let mut tx = pool.begin().await?;
    cratestack_sqlx::ensure_event_outbox_table(&mut *tx).await?;
    let row = update_returning(&mut *tx, &sql, &binds, pk, resolved).await?;
    if let Some(row) = &row {
        cratestack_sqlx::enqueue_event_outbox(&mut *tx, model, ModelEventKind::Updated, row)
            .await?;
    }
    tx.commit().await?;
    Ok(row)
}

pub(super) async fn delete_routed(
    schema: &Schema,
    pool: &sqlx_postgres::PgPool,
    model: &str,
    pk: &str,
) -> Result<Option<Row>, DataError> {
    let (resolved, info) = resolve_model(schema, model)?;
    let sql = build_delete_sql(&info);

    if !emitted_events(resolved).contains(&ModelEventKind::Deleted) {
        return delete_returning(pool, &sql, pk, resolved).await;
    }
    let mut tx = pool.begin().await?;
    cratestack_sqlx::ensure_event_outbox_table(&mut *tx).await?;
    let row = delete_returning(&mut *tx, &sql, pk, resolved).await?;
    if let Some(row) = &row {
        cratestack_sqlx::enqueue_event_outbox(&mut *tx, model, ModelEventKind::Deleted, row)
            .await?;
    }
    tx.commit().await?;
    Ok(row)
}
