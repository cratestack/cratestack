//! Batch primitives — `batch_get`, `batch_create`, `batch_update`,
//! `batch_delete`, `batch_upsert`.
//!
//! Wire shape is the tRPC-style envelope from `cratestack-core`: every
//! request returns `Vec<BatchItemResult<M>>` where each item carries an
//! independent `Ok(M)` or `Err(BatchItemError)`. The outer `Result` is
//! reserved for whole-batch infrastructure failures (size cap exceeded,
//! duplicate-input rejection, DB connection lost).
//!
//! Transactional model: one outer `BEGIN`, with each mutating item running
//! in a nested `SAVEPOINT`. Successful items commit together when the outer
//! transaction commits; per-item failures rollback to their savepoint, so
//! failed items leave no row, no audit row, no event outbox entry. The
//! two non-mutating ops (`batch_get`) and the single-statement op
//! (`batch_delete`) don't need savepoints — the WHERE clause already
//! filters out policy-denied / missing rows, and we walk the returned set
//! to produce the per-item envelope.
//!
//! Sizing: every request is capped at `BATCH_MAX_ITEMS` (1000) at the
//! outer guard. Duplicate-input keys are loud-failed at the same guard.

use std::collections::HashMap;
use std::hash::Hash;

use crate::sqlx;
// `Acquire::begin` is what gives us a nested transaction (= SAVEPOINT) on
// `&mut Transaction`. Without it in scope, `.begin()` resolves to the
// inherent `Transaction::begin` constructor and rustc rightly complains.
use sqlx_core::acquire::Acquire as _;

use cratestack_core::{
    AuditOperation, BATCH_MAX_ITEMS, BatchResponse, CoolContext, CoolError, ModelEventKind,
    find_duplicate_position,
};

use crate::{
    CreateModelInput, ModelDescriptor, ModelPrimaryKey, SqlValue, SqlxRuntime, UpdateModelInput,
    UpsertModelInput,
    audit::{build_audit_event, enqueue_audit_event, ensure_audit_table, fetch_for_audit},
    descriptor::{enqueue_event_outbox, ensure_event_outbox_table},
};

use super::support::{
    apply_create_defaults, evaluate_create_policies, find_column_value, push_action_policy_query,
    push_bind_value,
};

// ───── outer guards ─────────────────────────────────────────────────────────

fn validate_batch_size(len: usize) -> Result<(), CoolError> {
    if len > BATCH_MAX_ITEMS {
        return Err(CoolError::Validation(format!(
            "batch size {len} exceeds maximum of {BATCH_MAX_ITEMS}",
        )));
    }
    Ok(())
}

fn reject_duplicate_pks<K: Eq + Hash + Clone>(keys: &[K]) -> Result<(), CoolError> {
    if let Some((first, dup)) = find_duplicate_position(keys.iter().cloned()) {
        return Err(CoolError::Validation(format!(
            "duplicate primary key in batch at positions {first} and {dup}",
        )));
    }
    Ok(())
}

fn reject_duplicate_sql_values(values: &[SqlValue]) -> Result<(), CoolError> {
    if let Some((first, dup)) = cratestack_sql::find_duplicate_sql_value(values) {
        return Err(CoolError::Validation(format!(
            "duplicate primary key in batch at positions {first} and {dup}",
        )));
    }
    Ok(())
}

// ───── BatchGet ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct BatchGet<'a, M: 'static, PK: 'static> {
    pub(crate) runtime: &'a SqlxRuntime,
    pub(crate) descriptor: &'static ModelDescriptor<M, PK>,
    pub(crate) ids: Vec<PK>,
}

impl<'a, M: 'static, PK: 'static> BatchGet<'a, M, PK> {
    pub async fn run(self, ctx: &CoolContext) -> Result<BatchResponse<M>, CoolError>
    where
        for<'r> M:
            Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + ModelPrimaryKey<PK>,
        PK: Clone
            + Eq
            + Hash
            + Send
            + sqlx::Type<sqlx::Postgres>
            + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        validate_batch_size(self.ids.len())?;
        reject_duplicate_pks(&self.ids)?;
        if self.ids.is_empty() {
            return Ok(BatchResponse::from_results(vec![]));
        }

        // Single SELECT with IN-list + read policy + soft-delete filter.
        let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("SELECT ");
        query.push(self.descriptor.select_projection());
        query.push(" FROM ").push(self.descriptor.table_name);
        query.push(" WHERE ");
        if let Some(col) = self.descriptor.soft_delete_column {
            query.push(col).push(" IS NULL AND ");
        }
        query.push(self.descriptor.primary_key).push(" IN (");
        for (index, id) in self.ids.iter().enumerate() {
            if index > 0 {
                query.push(", ");
            }
            query.push_bind(id.clone());
        }
        query.push(") AND ");
        push_action_policy_query(
            &mut query,
            self.descriptor.read_allow_policies,
            self.descriptor.read_deny_policies,
            ctx,
        );

        let rows: Vec<M> = query
            .build_query_as::<M>()
            .fetch_all(self.runtime.pool())
            .await
            .map_err(|error| CoolError::Database(error.to_string()))?;

        // Walk-and-match: pair each input PK back to its row, or NotFound
        // when the read policy / soft-delete filter excluded it.
        let mut by_pk: HashMap<PK, M> =
            rows.into_iter().map(|m| (m.primary_key(), m)).collect();
        let per_item: Vec<Result<M, CoolError>> = self
            .ids
            .into_iter()
            .map(|id| {
                by_pk
                    .remove(&id)
                    .ok_or_else(|| CoolError::NotFound("no row matched".to_owned()))
            })
            .collect();

        Ok(BatchResponse::from_results(per_item))
    }
}

// ───── BatchDelete ──────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct BatchDelete<'a, M: 'static, PK: 'static> {
    pub(crate) runtime: &'a SqlxRuntime,
    pub(crate) descriptor: &'static ModelDescriptor<M, PK>,
    pub(crate) ids: Vec<PK>,
}

impl<'a, M: 'static, PK: 'static> BatchDelete<'a, M, PK> {
    pub async fn run(self, ctx: &CoolContext) -> Result<BatchResponse<M>, CoolError>
    where
        for<'r> M: Send
            + Unpin
            + sqlx::FromRow<'r, sqlx::postgres::PgRow>
            + ModelPrimaryKey<PK>
            + serde::Serialize,
        PK: Clone
            + Eq
            + Hash
            + Send
            + sqlx::Type<sqlx::Postgres>
            + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        validate_batch_size(self.ids.len())?;
        reject_duplicate_pks(&self.ids)?;
        if self.ids.is_empty() {
            return Ok(BatchResponse::from_results(vec![]));
        }

        let emits_event = self.descriptor.emits(ModelEventKind::Deleted);
        let audit_enabled = self.descriptor.audit_enabled;

        let mut tx = self
            .runtime
            .pool()
            .begin()
            .await
            .map_err(|error| CoolError::Database(error.to_string()))?;
        if emits_event {
            ensure_event_outbox_table(&mut *tx).await?;
        }
        if audit_enabled {
            ensure_audit_table(self.runtime.pool()).await?;
        }

        // Build the DELETE-or-soft-delete statement with the policy
        // predicate baked into the WHERE.
        let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("");
        match self.descriptor.soft_delete_column {
            Some(col) => {
                query.push("UPDATE ").push(self.descriptor.table_name);
                query.push(" SET ").push(col).push(" = NOW()");
                if let Some(version_col) = self.descriptor.version_column {
                    query
                        .push(", ")
                        .push(version_col)
                        .push(" = ")
                        .push(version_col)
                        .push(" + 1");
                }
                query.push(" WHERE ").push(col).push(" IS NULL AND ");
            }
            None => {
                query.push("DELETE FROM ").push(self.descriptor.table_name);
                query.push(" WHERE ");
            }
        }
        query.push(self.descriptor.primary_key).push(" IN (");
        for (index, id) in self.ids.iter().enumerate() {
            if index > 0 {
                query.push(", ");
            }
            query.push_bind(id.clone());
        }
        query.push(") AND ");
        push_action_policy_query(
            &mut query,
            self.descriptor.delete_allow_policies,
            self.descriptor.delete_deny_policies,
            ctx,
        );
        query
            .push(" RETURNING ")
            .push(self.descriptor.select_projection());

        let deleted: Vec<M> = query
            .build_query_as::<M>()
            .fetch_all(&mut *tx)
            .await
            .map_err(|error| CoolError::Database(error.to_string()))?;

        // Fan-out one audit + one outbox entry per actually-deleted row.
        // The RETURNING row IS the "before" snapshot — DELETE/soft-delete
        // returns the pre-mutation state.
        for record in &deleted {
            if emits_event {
                enqueue_event_outbox(
                    &mut *tx,
                    self.descriptor.schema_name,
                    ModelEventKind::Deleted,
                    record,
                )
                .await?;
            }
            if audit_enabled {
                let before = serde_json::to_value(record).ok();
                let event = build_audit_event(
                    self.descriptor,
                    AuditOperation::Delete,
                    before,
                    None,
                    ctx,
                );
                enqueue_audit_event(&mut *tx, &event).await?;
            }
        }

        tx.commit()
            .await
            .map_err(|error| CoolError::Database(error.to_string()))?;

        if emits_event {
            let _ = self.runtime.drain_event_outbox().await;
        }

        // Walk-and-match: any input id whose row isn't in `deleted` failed
        // the WHERE clause (already tombstoned, policy denied, or never
        // existed). All three collapse to NotFound on the wire.
        let mut by_pk: HashMap<PK, M> =
            deleted.into_iter().map(|m| (m.primary_key(), m)).collect();
        let per_item: Vec<Result<M, CoolError>> = self
            .ids
            .into_iter()
            .map(|id| {
                by_pk
                    .remove(&id)
                    .ok_or_else(|| CoolError::NotFound("no row matched".to_owned()))
            })
            .collect();

        Ok(BatchResponse::from_results(per_item))
    }
}

// ───── BatchCreate ──────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct BatchCreate<'a, M: 'static, PK: 'static, I> {
    pub(crate) runtime: &'a SqlxRuntime,
    pub(crate) descriptor: &'static ModelDescriptor<M, PK>,
    pub(crate) inputs: Vec<I>,
}

impl<'a, M: 'static, PK: 'static, I> BatchCreate<'a, M, PK, I>
where
    I: CreateModelInput<M> + Send,
{
    pub async fn run(self, ctx: &CoolContext) -> Result<BatchResponse<M>, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
    {
        validate_batch_size(self.inputs.len())?;
        // No PK dedup here — `CreateModelInput` doesn't expose the PK
        // generically (and server-generated PKs make duplicates impossible).
        // Client-supplied PK collisions trip the DB uniqueness constraint
        // and surface per-item as `CoolError::Database`. The right primitive
        // for idempotent client-PK ingestion is `.batch_upsert(...)`.
        if self.inputs.is_empty() {
            return Ok(BatchResponse::from_results(vec![]));
        }

        let emits_event = self.descriptor.emits(ModelEventKind::Created);
        let audit_enabled = self.descriptor.audit_enabled;

        let mut tx = self
            .runtime
            .pool()
            .begin()
            .await
            .map_err(|error| CoolError::Database(error.to_string()))?;
        if emits_event {
            ensure_event_outbox_table(&mut *tx).await?;
        }
        if audit_enabled {
            ensure_audit_table(self.runtime.pool()).await?;
        }

        let mut per_item: Vec<Result<M, CoolError>> = Vec::with_capacity(self.inputs.len());
        for input in self.inputs {
            let outcome = run_create_item(
                &mut tx,
                self.runtime.pool(),
                self.descriptor,
                input,
                ctx,
                emits_event,
                audit_enabled,
            )
            .await?;
            per_item.push(outcome);
        }

        tx.commit()
            .await
            .map_err(|error| CoolError::Database(error.to_string()))?;

        if emits_event {
            let _ = self.runtime.drain_event_outbox().await;
        }

        Ok(BatchResponse::from_results(per_item))
    }
}

async fn run_create_item<'tx, M, PK, I>(
    outer: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
    policy_pool: &sqlx::PgPool,
    descriptor: &'static ModelDescriptor<M, PK>,
    input: I,
    ctx: &CoolContext,
    emits_event: bool,
    audit_enabled: bool,
) -> Result<Result<M, CoolError>, CoolError>
where
    I: CreateModelInput<M>,
    for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
{
    let mut item_tx = outer
        .begin()
        .await
        .map_err(|error| CoolError::Database(error.to_string()))?;

    // All per-item failures funnel through this inner closure so the
    // savepoint commit/rollback decision is centralized below.
    let inner: Result<M, CoolError> = async {
        input.validate()?;
        let mut values = apply_create_defaults(input.sql_values(), descriptor.create_defaults, ctx)?;
        if let Some(version_col) = descriptor.version_column
            && find_column_value(&values, version_col).is_none()
        {
            values.push(crate::SqlColumnValue {
                column: version_col,
                value: crate::SqlValue::Int(0),
            });
        }
        if values.is_empty() {
            return Err(CoolError::Validation(
                "create input must contain at least one column".to_owned(),
            ));
        }
        if !evaluate_create_policies(
            policy_pool,
            descriptor.create_allow_policies,
            descriptor.create_deny_policies,
            &values,
            ctx,
        )
        .await?
        {
            return Err(CoolError::Forbidden(
                "create policy denied this operation".to_owned(),
            ));
        }

        let record = insert_one_into_savepoint::<M, PK>(&mut item_tx, descriptor, &values).await?;

        if emits_event {
            enqueue_event_outbox(
                &mut *item_tx,
                descriptor.schema_name,
                ModelEventKind::Created,
                &record,
            )
            .await?;
        }
        if audit_enabled {
            let after = serde_json::to_value(&record).ok();
            let event =
                build_audit_event(descriptor, AuditOperation::Create, None, after, ctx);
            enqueue_audit_event(&mut *item_tx, &event).await?;
        }
        Ok(record)
    }
    .await;

    match inner {
        Ok(record) => {
            item_tx
                .commit()
                .await
                .map_err(|error| CoolError::Database(error.to_string()))?;
            Ok(Ok(record))
        }
        Err(item_err) => {
            // ROLLBACK TO SAVEPOINT brings the outer tx back to its
            // pre-savepoint state. If THAT fails, the outer tx is dead and
            // we propagate as the outer `Result::Err` — no point trying to
            // continue.
            item_tx
                .rollback()
                .await
                .map_err(|error| CoolError::Database(error.to_string()))?;
            Ok(Err(item_err))
        }
    }
}

async fn insert_one_into_savepoint<'tx, M, PK>(
    executor: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
    descriptor: &'static ModelDescriptor<M, PK>,
    values: &[crate::SqlColumnValue],
) -> Result<M, CoolError>
where
    for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow>,
{
    let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("INSERT INTO ");
    query.push(descriptor.table_name).push(" (");
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            query.push(", ");
        }
        query.push(value.column);
    }
    query.push(") VALUES (");
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            query.push(", ");
        }
        push_bind_value(&mut query, &value.value);
    }
    query
        .push(") RETURNING ")
        .push(descriptor.select_projection());

    query
        .build_query_as::<M>()
        .fetch_one(&mut **executor)
        .await
        .map_err(|error| classify_insert_error(error))
}

/// Map a sqlx error from a per-item INSERT into the right `CoolError`
/// variant. Unique-constraint violations become `Conflict` so the envelope
/// surfaces the right code; everything else stays `Database`.
fn classify_insert_error(error: sqlx::Error) -> CoolError {
    if let sqlx::Error::Database(db_err) = &error
        && let Some(code) = db_err.code()
        && code == "23505"
    {
        return CoolError::Conflict(db_err.message().to_owned());
    }
    CoolError::Database(error.to_string())
}

// ───── BatchUpdate ──────────────────────────────────────────────────────────

/// One per-item update: `(id, patch, optional expected version)`.
pub type BatchUpdateItem<PK, I> = (PK, I, Option<i64>);

#[derive(Debug, Clone)]
pub struct BatchUpdate<'a, M: 'static, PK: 'static, I> {
    pub(crate) runtime: &'a SqlxRuntime,
    pub(crate) descriptor: &'static ModelDescriptor<M, PK>,
    pub(crate) items: Vec<BatchUpdateItem<PK, I>>,
}

impl<'a, M: 'static, PK: 'static, I> BatchUpdate<'a, M, PK, I>
where
    I: UpdateModelInput<M> + Send,
{
    pub async fn run(self, ctx: &CoolContext) -> Result<BatchResponse<M>, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
        PK: Clone
            + Eq
            + Hash
            + Send
            + sqlx::Type<sqlx::Postgres>
            + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        validate_batch_size(self.items.len())?;
        let ids: Vec<PK> = self.items.iter().map(|(id, _, _)| id.clone()).collect();
        reject_duplicate_pks(&ids)?;
        if self.items.is_empty() {
            return Ok(BatchResponse::from_results(vec![]));
        }

        let emits_event = self.descriptor.emits(ModelEventKind::Updated);
        let audit_enabled = self.descriptor.audit_enabled;

        let mut tx = self
            .runtime
            .pool()
            .begin()
            .await
            .map_err(|error| CoolError::Database(error.to_string()))?;
        if emits_event {
            ensure_event_outbox_table(&mut *tx).await?;
        }
        if audit_enabled {
            ensure_audit_table(self.runtime.pool()).await?;
        }

        let mut per_item: Vec<Result<M, CoolError>> = Vec::with_capacity(self.items.len());
        for (id, input, if_match) in self.items {
            let outcome = run_update_item(
                &mut tx,
                self.descriptor,
                id,
                input,
                if_match,
                ctx,
                emits_event,
                audit_enabled,
            )
            .await?;
            per_item.push(outcome);
        }

        tx.commit()
            .await
            .map_err(|error| CoolError::Database(error.to_string()))?;

        if emits_event {
            let _ = self.runtime.drain_event_outbox().await;
        }

        Ok(BatchResponse::from_results(per_item))
    }
}

async fn run_update_item<'tx, M, PK, I>(
    outer: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
    descriptor: &'static ModelDescriptor<M, PK>,
    id: PK,
    input: I,
    if_match: Option<i64>,
    ctx: &CoolContext,
    emits_event: bool,
    audit_enabled: bool,
) -> Result<Result<M, CoolError>, CoolError>
where
    I: UpdateModelInput<M>,
    PK: Clone + Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
{
    let mut item_tx = outer
        .begin()
        .await
        .map_err(|error| CoolError::Database(error.to_string()))?;

    let inner: Result<M, CoolError> = async {
        if descriptor.version_column.is_some() && if_match.is_none() {
            return Err(CoolError::PreconditionFailed(
                "If-Match required for versioned model".to_owned(),
            ));
        }
        input.validate()?;
        let values = input.sql_values();
        if values.is_empty() {
            return Err(CoolError::Validation(
                "update input must contain at least one changed column".to_owned(),
            ));
        }

        // Capture before-snapshot under FOR UPDATE for clean audit timing.
        let before = if audit_enabled {
            fetch_for_audit(&mut *item_tx, descriptor, id.clone()).await?
        } else {
            None
        };

        let record = update_one_in_savepoint(
            &mut item_tx,
            descriptor,
            id,
            &values,
            ctx,
            if_match,
        )
        .await?;

        if emits_event {
            enqueue_event_outbox(
                &mut *item_tx,
                descriptor.schema_name,
                ModelEventKind::Updated,
                &record,
            )
            .await?;
        }
        if audit_enabled {
            let before_snapshot = before.as_ref().and_then(|m| serde_json::to_value(m).ok());
            let after = serde_json::to_value(&record).ok();
            let event = build_audit_event(
                descriptor,
                AuditOperation::Update,
                before_snapshot,
                after,
                ctx,
            );
            enqueue_audit_event(&mut *item_tx, &event).await?;
        }
        Ok(record)
    }
    .await;

    match inner {
        Ok(record) => {
            item_tx
                .commit()
                .await
                .map_err(|error| CoolError::Database(error.to_string()))?;
            Ok(Ok(record))
        }
        Err(item_err) => {
            item_tx
                .rollback()
                .await
                .map_err(|error| CoolError::Database(error.to_string()))?;
            Ok(Err(item_err))
        }
    }
}

async fn update_one_in_savepoint<'tx, M, PK>(
    executor: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
    descriptor: &'static ModelDescriptor<M, PK>,
    id: PK,
    values: &[crate::SqlColumnValue],
    ctx: &CoolContext,
    if_match: Option<i64>,
) -> Result<M, CoolError>
where
    for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow>,
    PK: Clone + Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
{
    let version_column = descriptor.version_column;
    let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("UPDATE ");
    query.push(descriptor.table_name).push(" SET ");
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            query.push(", ");
        }
        query.push(value.column).push(" = ");
        push_bind_value(&mut query, &value.value);
    }
    if let Some(version_col) = version_column {
        query
            .push(", ")
            .push(version_col)
            .push(" = ")
            .push(version_col)
            .push(" + 1");
    }
    query
        .push(" WHERE ")
        .push(descriptor.primary_key)
        .push(" = ");
    query.push_bind(id);
    if let (Some(version_col), Some(expected)) = (version_column, if_match) {
        query.push(" AND ").push(version_col).push(" = ");
        query.push_bind(expected);
    }
    query.push(" AND ");
    push_action_policy_query(
        &mut query,
        descriptor.update_allow_policies,
        descriptor.update_deny_policies,
        ctx,
    );
    query
        .push(" RETURNING ")
        .push(descriptor.select_projection());

    let outcome = query
        .build_query_as::<M>()
        .fetch_optional(&mut **executor)
        .await
        .map_err(|error| CoolError::Database(error.to_string()))?;
    match outcome {
        Some(record) => Ok(record),
        None => {
            // Could be: row missing, policy denied, version mismatch, soft-
            // deleted. Probing to discriminate adds round-trips; we report
            // Forbidden for batches (matches single-update behavior) when
            // there's no `if_match`, and PreconditionFailed when there is.
            // Either way the caller's recovery is the same: refetch & retry.
            if if_match.is_some() {
                Err(CoolError::PreconditionFailed(
                    "version mismatch or row missing".to_owned(),
                ))
            } else {
                Err(CoolError::Forbidden(
                    "update policy denied or row missing".to_owned(),
                ))
            }
        }
    }
}

// ───── BatchUpsert ──────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct BatchUpsert<'a, M: 'static, PK: 'static, I> {
    pub(crate) runtime: &'a SqlxRuntime,
    pub(crate) descriptor: &'static ModelDescriptor<M, PK>,
    pub(crate) inputs: Vec<I>,
}

impl<'a, M: 'static, PK: 'static, I> BatchUpsert<'a, M, PK, I>
where
    I: UpsertModelInput<M>,
{
    pub async fn run(self, ctx: &CoolContext) -> Result<BatchResponse<M>, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
        PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        validate_batch_size(self.inputs.len())?;
        // Upsert dedup runs on the per-input primary key — this is what
        // keeps two callers from both producing batches with the same key
        // and ending up with surprising "second write wins" semantics.
        let pks: Vec<SqlValue> = self
            .inputs
            .iter()
            .map(UpsertModelInput::primary_key_value)
            .collect();
        reject_duplicate_sql_values(&pks)?;
        if self.inputs.is_empty() {
            return Ok(BatchResponse::from_results(vec![]));
        }

        let emits_created = self.descriptor.emits(ModelEventKind::Created);
        let emits_updated = self.descriptor.emits(ModelEventKind::Updated);
        let audit_enabled = self.descriptor.audit_enabled;

        let mut tx = self
            .runtime
            .pool()
            .begin()
            .await
            .map_err(|error| CoolError::Database(error.to_string()))?;
        if emits_created || emits_updated {
            ensure_event_outbox_table(&mut *tx).await?;
        }
        if audit_enabled {
            ensure_audit_table(self.runtime.pool()).await?;
        }

        let mut per_item: Vec<Result<M, CoolError>> = Vec::with_capacity(self.inputs.len());
        for input in self.inputs {
            let outcome = run_upsert_item(
                &mut tx,
                self.runtime.pool(),
                self.descriptor,
                input,
                ctx,
                emits_created,
                emits_updated,
                audit_enabled,
            )
            .await?;
            per_item.push(outcome);
        }

        tx.commit()
            .await
            .map_err(|error| CoolError::Database(error.to_string()))?;

        if emits_created || emits_updated {
            let _ = self.runtime.drain_event_outbox().await;
        }

        Ok(BatchResponse::from_results(per_item))
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_upsert_item<'tx, M, PK, I>(
    outer: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
    policy_pool: &sqlx::PgPool,
    descriptor: &'static ModelDescriptor<M, PK>,
    input: I,
    ctx: &CoolContext,
    emits_created: bool,
    emits_updated: bool,
    audit_enabled: bool,
) -> Result<Result<M, CoolError>, CoolError>
where
    I: UpsertModelInput<M>,
    PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
{
    let mut item_tx = outer
        .begin()
        .await
        .map_err(|error| CoolError::Database(error.to_string()))?;

    let inner: Result<M, CoolError> = async {
        input.validate()?;
        let mut insert_values =
            apply_create_defaults(input.sql_values(), descriptor.create_defaults, ctx)?;
        if let Some(version_col) = descriptor.version_column
            && find_column_value(&insert_values, version_col).is_none()
        {
            insert_values.push(crate::SqlColumnValue {
                column: version_col,
                value: crate::SqlValue::Int(0),
            });
        }
        if insert_values.is_empty() {
            return Err(CoolError::Validation(
                "upsert input must contain at least one column".to_owned(),
            ));
        }
        if !evaluate_create_policies(
            policy_pool,
            descriptor.create_allow_policies,
            descriptor.create_deny_policies,
            &insert_values,
            ctx,
        )
        .await?
        {
            return Err(CoolError::Forbidden(
                "create policy denied this upsert".to_owned(),
            ));
        }

        let pk_value = input.primary_key_value();
        // Probe under FOR UPDATE so the audit before-snapshot is consistent
        // with the row state at the moment of the upsert.
        let before_record =
            select_for_update_by_pk_value(&mut item_tx, descriptor, &pk_value).await?;
        let inserted = before_record.is_none();

        if !inserted
            && !row_passes_update_policy(policy_pool, descriptor, &pk_value, ctx).await?
        {
            return Err(CoolError::Forbidden(
                "update policy denied this upsert".to_owned(),
            ));
        }

        let before_snapshot = if !inserted && audit_enabled {
            before_record
                .as_ref()
                .and_then(|m| serde_json::to_value(m).ok())
        } else {
            None
        };

        let record =
            upsert_one_in_savepoint::<M, PK>(&mut item_tx, descriptor, &insert_values).await?;

        let event_kind = if inserted {
            ModelEventKind::Created
        } else {
            ModelEventKind::Updated
        };
        let audit_op = if inserted {
            AuditOperation::Create
        } else {
            AuditOperation::Update
        };
        let emits_this_event = if inserted { emits_created } else { emits_updated };

        if emits_this_event {
            enqueue_event_outbox(&mut *item_tx, descriptor.schema_name, event_kind, &record)
                .await?;
        }
        if audit_enabled {
            let after = serde_json::to_value(&record).ok();
            let event = build_audit_event(descriptor, audit_op, before_snapshot, after, ctx);
            enqueue_audit_event(&mut *item_tx, &event).await?;
        }

        Ok(record)
    }
    .await;

    match inner {
        Ok(record) => {
            item_tx
                .commit()
                .await
                .map_err(|error| CoolError::Database(error.to_string()))?;
            Ok(Ok(record))
        }
        Err(item_err) => {
            item_tx
                .rollback()
                .await
                .map_err(|error| CoolError::Database(error.to_string()))?;
            Ok(Err(item_err))
        }
    }
}

async fn select_for_update_by_pk_value<'tx, M, PK>(
    executor: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
    descriptor: &'static ModelDescriptor<M, PK>,
    pk_value: &SqlValue,
) -> Result<Option<M>, CoolError>
where
    for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow>,
{
    let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("SELECT ");
    query.push(descriptor.select_projection());
    query.push(" FROM ").push(descriptor.table_name);
    query
        .push(" WHERE ")
        .push(descriptor.primary_key)
        .push(" = ");
    push_bind_value(&mut query, pk_value);
    if let Some(col) = descriptor.soft_delete_column {
        query.push(" AND ").push(col).push(" IS NULL");
    }
    query.push(" FOR UPDATE");

    query
        .build_query_as::<M>()
        .fetch_optional(&mut **executor)
        .await
        .map_err(|error| CoolError::Database(error.to_string()))
}

async fn row_passes_update_policy<M, PK>(
    policy_pool: &sqlx::PgPool,
    descriptor: &'static ModelDescriptor<M, PK>,
    pk_value: &SqlValue,
    ctx: &CoolContext,
) -> Result<bool, CoolError> {
    let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("SELECT 1 FROM ");
    query.push(descriptor.table_name);
    query
        .push(" WHERE ")
        .push(descriptor.primary_key)
        .push(" = ");
    push_bind_value(&mut query, pk_value);
    query.push(" AND ");
    push_action_policy_query(
        &mut query,
        descriptor.update_allow_policies,
        descriptor.update_deny_policies,
        ctx,
    );

    let row: Option<(i32,)> = query
        .build_query_as::<(i32,)>()
        .fetch_optional(policy_pool)
        .await
        .map_err(|error| CoolError::Database(error.to_string()))?;
    Ok(row.is_some())
}

async fn upsert_one_in_savepoint<'tx, M, PK>(
    executor: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
    descriptor: &'static ModelDescriptor<M, PK>,
    insert_values: &[crate::SqlColumnValue],
) -> Result<M, CoolError>
where
    for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow>,
{
    let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("INSERT INTO ");
    query.push(descriptor.table_name).push(" (");
    for (index, value) in insert_values.iter().enumerate() {
        if index > 0 {
            query.push(", ");
        }
        query.push(value.column);
    }
    query.push(") VALUES (");
    for (index, value) in insert_values.iter().enumerate() {
        if index > 0 {
            query.push(", ");
        }
        push_bind_value(&mut query, &value.value);
    }
    query
        .push(") ON CONFLICT (")
        .push(descriptor.primary_key)
        .push(") DO UPDATE SET ");

    if descriptor.upsert_update_columns.is_empty() {
        query.push(descriptor.primary_key);
        query.push(" = EXCLUDED.").push(descriptor.primary_key);
    } else {
        for (index, column) in descriptor.upsert_update_columns.iter().enumerate() {
            if index > 0 {
                query.push(", ");
            }
            query.push(*column).push(" = EXCLUDED.").push(*column);
        }
    }
    if let Some(version_col) = descriptor.version_column {
        query
            .push(", ")
            .push(version_col)
            .push(" = ")
            .push(descriptor.table_name)
            .push(".")
            .push(version_col)
            .push(" + 1");
    }

    query
        .push(" RETURNING ")
        .push(descriptor.select_projection());

    query
        .build_query_as::<M>()
        .fetch_one(&mut **executor)
        .await
        .map_err(|error| CoolError::Database(error.to_string()))
}
