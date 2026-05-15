use crate::sqlx;

use cratestack_core::{AuditOperation, BatchSummary, CoolContext, CoolError, ModelEventKind};

use crate::{
    CreateModelInput, FilterExpr, ModelDescriptor, SqlValue, SqlxRuntime, UpdateModelInput,
    UpsertModelInput,
    audit::{build_audit_event, enqueue_audit_event, ensure_audit_table, fetch_for_audit},
    descriptor::{enqueue_event_outbox, ensure_event_outbox_table},
};

use super::support::{
    apply_create_defaults, evaluate_create_policies, find_column_value, push_action_policy_query,
    push_bind_value, push_filter_query,
};

/// Render the preview SQL for a bulk update-by-predicate. Pure helper, no
/// I/O — separated so the soft-delete / version / filter-policy branches
/// can be unit-tested without a runtime. The output is a *sketch*: the
/// filter and policy clauses are placeholders (`<filters>`,
/// `<update_policy>`) because the live SQL is built via sqlx's
/// `QueryBuilder` and would inline binds. Migration tooling and the
/// schema studio call this to surface the rough shape of the statement
/// without needing real auth context.
pub fn render_update_many_preview_sql(
    table_name: &str,
    has_soft_delete: bool,
    version_column: Option<&str>,
    set_columns: &[&str],
    select_projection: &str,
) -> String {
    let mut sql = format!("UPDATE {table_name} SET ");
    for (idx, column) in set_columns.iter().enumerate() {
        if idx > 0 {
            sql.push_str(", ");
        }
        sql.push_str(&format!("{column} = ${}", idx + 1));
    }
    if let Some(version_col) = version_column {
        sql.push_str(&format!(", {version_col} = {version_col} + 1"));
    }
    sql.push_str(" WHERE ");
    if has_soft_delete {
        sql.push_str("<soft_delete IS NULL> AND ");
    }
    sql.push_str("<filters> AND <update_policy> RETURNING ");
    sql.push_str(select_projection);
    sql
}

/// Render the SQL string for an update. Pure helper, no I/O — separated
/// so the version-aware branch can be unit-tested without a runtime.
pub fn render_update_preview_sql(
    table_name: &str,
    primary_key: &str,
    version_column: Option<&str>,
    columns: &[&str],
    select_projection: &str,
) -> String {
    let assignments = columns
        .iter()
        .enumerate()
        .map(|(index, column)| format!("{column} = ${}", index + 1))
        .collect::<Vec<_>>()
        .join(", ");

    match version_column {
        Some(version_col) => format!(
            "UPDATE {} SET {}, {} = {} + 1 WHERE {} = ${} AND {} = ${} RETURNING {}",
            table_name,
            assignments,
            version_col,
            version_col,
            primary_key,
            columns.len() + 1,
            version_col,
            columns.len() + 2,
            select_projection,
        ),
        None => format!(
            "UPDATE {} SET {} WHERE {} = ${} RETURNING {}",
            table_name,
            assignments,
            primary_key,
            columns.len() + 1,
            select_projection,
        ),
    }
}

#[derive(Debug, Clone)]
pub struct CreateRecord<'a, M: 'static, PK: 'static, I> {
    pub(crate) runtime: &'a SqlxRuntime,
    pub(crate) descriptor: &'static ModelDescriptor<M, PK>,
    pub(crate) input: I,
}

impl<'a, M: 'static, PK: 'static, I> CreateRecord<'a, M, PK, I>
where
    I: CreateModelInput<M>,
{
    pub fn preview_sql(&self) -> String {
        let values = self.input.sql_values();
        let placeholders = (1..=values.len())
            .map(|index| format!("${index}"))
            .collect::<Vec<_>>()
            .join(", ");
        let columns = values
            .iter()
            .map(|value| value.column)
            .collect::<Vec<_>>()
            .join(", ");

        format!(
            "INSERT INTO {} ({}) VALUES ({}) RETURNING {}",
            self.descriptor.table_name,
            columns,
            placeholders,
            self.descriptor.select_projection(),
        )
    }

    /// Like [`Self::run`] but participates in a caller-supplied transaction.
    /// The insert + outbox enqueue + audit write all happen inside `tx`;
    /// the caller is responsible for `tx.commit()` / `tx.rollback()`. The
    /// event outbox is *not* drained — the outbox row isn't visible to the
    /// drain worker until the caller commits.
    pub async fn run_in_tx<'tx>(
        self,
        tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
        ctx: &CoolContext,
    ) -> Result<M, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
    {
        let emits_event = self.descriptor.emits(ModelEventKind::Created);
        let audit_enabled = self.descriptor.audit_enabled;
        if emits_event {
            ensure_event_outbox_table(&mut **tx).await?;
        }
        if audit_enabled {
            ensure_audit_table(self.runtime.pool()).await?;
        }
        let record = create_record_with_executor(
            &mut **tx,
            self.runtime.pool(),
            self.descriptor,
            self.input,
            ctx,
        )
        .await?;
        if emits_event {
            enqueue_event_outbox(
                &mut **tx,
                self.descriptor.schema_name,
                ModelEventKind::Created,
                &record,
            )
            .await?;
        }
        if audit_enabled {
            let after = serde_json::to_value(&record).ok();
            let event =
                build_audit_event(self.descriptor, AuditOperation::Create, None, after, ctx);
            enqueue_audit_event(&mut **tx, &event).await?;
        }
        Ok(record)
    }

    pub async fn run(self, ctx: &CoolContext) -> Result<M, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
    {
        let emits_event = self.descriptor.emits(ModelEventKind::Created);
        let audit_enabled = self.descriptor.audit_enabled;
        let needs_tx = emits_event || audit_enabled;
        let record = if needs_tx {
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
            let record = create_record_with_executor(
                &mut *tx,
                self.runtime.pool(),
                self.descriptor,
                self.input,
                ctx,
            )
            .await?;
            if emits_event {
                enqueue_event_outbox(
                    &mut *tx,
                    self.descriptor.schema_name,
                    ModelEventKind::Created,
                    &record,
                )
                .await?;
            }
            if audit_enabled {
                let after = serde_json::to_value(&record).ok();
                let event =
                    build_audit_event(self.descriptor, AuditOperation::Create, None, after, ctx);
                enqueue_audit_event(&mut *tx, &event).await?;
            }
            tx.commit()
                .await
                .map_err(|error| CoolError::Database(error.to_string()))?;
            record
        } else {
            create_record_with_executor(
                self.runtime.pool(),
                self.runtime.pool(),
                self.descriptor,
                self.input,
                ctx,
            )
            .await?
        };

        if emits_event {
            let _ = self.runtime.drain_event_outbox().await;
        }

        Ok(record)
    }
}

#[derive(Debug, Clone)]
pub struct UpdateRecord<'a, M: 'static, PK: 'static> {
    pub(crate) runtime: &'a SqlxRuntime,
    pub(crate) descriptor: &'static ModelDescriptor<M, PK>,
    pub(crate) id: PK,
}

impl<'a, M: 'static, PK: 'static> UpdateRecord<'a, M, PK> {
    pub fn set<I>(self, input: I) -> UpdateRecordSet<'a, M, PK, I> {
        UpdateRecordSet {
            runtime: self.runtime,
            descriptor: self.descriptor,
            id: self.id,
            input,
            if_match: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct UpdateRecordSet<'a, M: 'static, PK: 'static, I> {
    pub(crate) runtime: &'a SqlxRuntime,
    pub(crate) descriptor: &'static ModelDescriptor<M, PK>,
    pub(crate) id: PK,
    pub(crate) input: I,
    pub(crate) if_match: Option<i64>,
}

impl<'a, M: 'static, PK: 'static, I> UpdateRecordSet<'a, M, PK, I>
where
    I: UpdateModelInput<M>,
{
    /// Attach an expected version for optimistic locking. The update will only
    /// succeed if the row's current `@version` field matches `expected`.
    /// Required on models that declare `@version`; ignored otherwise.
    pub fn if_match(mut self, expected: i64) -> Self {
        self.if_match = Some(expected);
        self
    }

    pub fn preview_sql(&self) -> String {
        let values = self.input.sql_values();
        let columns: Vec<&str> = values.iter().map(|v| v.column).collect();
        render_update_preview_sql(
            self.descriptor.table_name,
            self.descriptor.primary_key,
            self.descriptor.version_column,
            &columns,
            &self.descriptor.select_projection(),
        )
    }

    /// Like [`Self::run`] but participates in a caller-supplied transaction.
    /// See [`CreateRecord::run_in_tx`] for the contract; in short: the
    /// update + outbox + audit writes happen in `tx`; caller commits; the
    /// outbox drain runs whenever the caller next calls
    /// `runtime.drain_event_outbox()`.
    pub async fn run_in_tx<'tx>(
        self,
        tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
        ctx: &CoolContext,
    ) -> Result<M, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
        PK: Send + Clone + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        if self.descriptor.version_column.is_some() && self.if_match.is_none() {
            return Err(CoolError::PreconditionFailed(
                "If-Match header required for versioned model".to_owned(),
            ));
        }
        let emits_event = self.descriptor.emits(ModelEventKind::Updated);
        let audit_enabled = self.descriptor.audit_enabled;
        if emits_event {
            ensure_event_outbox_table(&mut **tx).await?;
        }
        if audit_enabled {
            ensure_audit_table(self.runtime.pool()).await?;
        }
        let before_record = if audit_enabled {
            fetch_for_audit(&mut **tx, self.descriptor, self.id.clone()).await?
        } else {
            None
        };
        let before_snapshot = before_record
            .as_ref()
            .and_then(|m| serde_json::to_value(m).ok());
        let record = update_record_with_executor(
            &mut **tx,
            self.runtime.pool(),
            self.descriptor,
            self.id,
            self.input,
            ctx,
            self.if_match,
        )
        .await?;
        if emits_event {
            enqueue_event_outbox(
                &mut **tx,
                self.descriptor.schema_name,
                ModelEventKind::Updated,
                &record,
            )
            .await?;
        }
        if audit_enabled {
            let after = serde_json::to_value(&record).ok();
            let event = build_audit_event(
                self.descriptor,
                AuditOperation::Update,
                before_snapshot,
                after,
                ctx,
            );
            enqueue_audit_event(&mut **tx, &event).await?;
        }
        Ok(record)
    }

    pub async fn run(self, ctx: &CoolContext) -> Result<M, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
        PK: Send + Clone + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        if self.descriptor.version_column.is_some() && self.if_match.is_none() {
            return Err(CoolError::PreconditionFailed(
                "If-Match header required for versioned model".to_owned(),
            ));
        }
        let emits_event = self.descriptor.emits(ModelEventKind::Updated);
        let audit_enabled = self.descriptor.audit_enabled;
        let needs_tx = emits_event || audit_enabled;
        let record = if needs_tx {
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
            // Capture the BEFORE snapshot under a row-level lock so concurrent
            // mutations can't race the audit.
            let before_record = if audit_enabled {
                fetch_for_audit(&mut *tx, self.descriptor, self.id.clone()).await?
            } else {
                None
            };
            let before_snapshot = before_record
                .as_ref()
                .and_then(|m| serde_json::to_value(m).ok());
            let record = update_record_with_executor(
                &mut *tx,
                self.runtime.pool(),
                self.descriptor,
                self.id,
                self.input,
                ctx,
                self.if_match,
            )
            .await?;
            if emits_event {
                enqueue_event_outbox(
                    &mut *tx,
                    self.descriptor.schema_name,
                    ModelEventKind::Updated,
                    &record,
                )
                .await?;
            }
            if audit_enabled {
                let after = serde_json::to_value(&record).ok();
                let event = build_audit_event(
                    self.descriptor,
                    AuditOperation::Update,
                    before_snapshot,
                    after,
                    ctx,
                );
                enqueue_audit_event(&mut *tx, &event).await?;
            }
            tx.commit()
                .await
                .map_err(|error| CoolError::Database(error.to_string()))?;
            record
        } else {
            update_record_with_executor(
                self.runtime.pool(),
                self.runtime.pool(),
                self.descriptor,
                self.id,
                self.input,
                ctx,
                self.if_match,
            )
            .await?
        };

        if emits_event {
            let _ = self.runtime.drain_event_outbox().await;
        }

        Ok(record)
    }
}

#[derive(Debug, Clone)]
pub struct DeleteRecord<'a, M: 'static, PK: 'static> {
    pub(crate) runtime: &'a SqlxRuntime,
    pub(crate) descriptor: &'static ModelDescriptor<M, PK>,
    pub(crate) id: PK,
}

impl<'a, M: 'static, PK: 'static> DeleteRecord<'a, M, PK> {
    pub fn preview_sql(&self) -> String {
        format!(
            "DELETE FROM {} WHERE {} = $1 RETURNING {}",
            self.descriptor.table_name,
            self.descriptor.primary_key,
            self.descriptor.select_projection(),
        )
    }

    /// Like [`Self::run`] but participates in a caller-supplied transaction.
    /// See [`CreateRecord::run_in_tx`].
    pub async fn run_in_tx<'tx>(
        self,
        tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
        ctx: &CoolContext,
    ) -> Result<M, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
        PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        let emits_event = self.descriptor.emits(ModelEventKind::Deleted);
        let audit_enabled = self.descriptor.audit_enabled;
        if emits_event {
            ensure_event_outbox_table(&mut **tx).await?;
        }
        if audit_enabled {
            ensure_audit_table(self.runtime.pool()).await?;
        }
        let record = delete_returning_record(&mut **tx, self.descriptor, self.id, ctx).await?;
        if emits_event {
            enqueue_event_outbox(
                &mut **tx,
                self.descriptor.schema_name,
                ModelEventKind::Deleted,
                &record,
            )
            .await?;
        }
        if audit_enabled {
            let before = serde_json::to_value(&record).ok();
            let event =
                build_audit_event(self.descriptor, AuditOperation::Delete, before, None, ctx);
            enqueue_audit_event(&mut **tx, &event).await?;
        }
        Ok(record)
    }

    pub async fn run(self, ctx: &CoolContext) -> Result<M, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
        PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        let emits_event = self.descriptor.emits(ModelEventKind::Deleted);
        let audit_enabled = self.descriptor.audit_enabled;
        let needs_tx = emits_event || audit_enabled;
        let record = if needs_tx {
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

            let record = delete_returning_record(&mut *tx, self.descriptor, self.id, ctx).await?;
            if emits_event {
                enqueue_event_outbox(
                    &mut *tx,
                    self.descriptor.schema_name,
                    ModelEventKind::Deleted,
                    &record,
                )
                .await?;
            }
            if audit_enabled {
                // DELETE ... RETURNING yields the row's pre-delete state, so
                // it doubles as the audit `before` snapshot.
                let before = serde_json::to_value(&record).ok();
                let event =
                    build_audit_event(self.descriptor, AuditOperation::Delete, before, None, ctx);
                enqueue_audit_event(&mut *tx, &event).await?;
            }
            tx.commit()
                .await
                .map_err(|error| CoolError::Database(error.to_string()))?;
            record
        } else {
            delete_returning_record(self.runtime.pool(), self.descriptor, self.id, ctx).await?
        };

        if emits_event {
            let _ = self.runtime.drain_event_outbox().await;
        }

        Ok(record)
    }
}

pub async fn create_record_with_executor<'e, E, M, PK, I>(
    executor: E,
    policy_pool: &sqlx::PgPool,
    descriptor: &'static ModelDescriptor<M, PK>,
    input: I,
    ctx: &CoolContext,
) -> Result<M, CoolError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    I: CreateModelInput<M>,
    for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
{
    input.validate()?;
    let mut values = apply_create_defaults(input.sql_values(), descriptor.create_defaults, ctx)?;
    // Seed the optimistic-lock column server-side. `@version` is excluded
    // from the generated Create input so clients can't pick the initial
    // value, and the column has no SQL `DEFAULT`. If we didn't write it
    // here, the INSERT would either skip the column (only valid when the
    // DB-level default is set, which we don't require) or fail under
    // `NOT NULL`. Done after `apply_create_defaults` so @default-driven
    // overrides still win if a schema ever lands one.
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

    insert_returning_record(executor, descriptor, &values).await
}

pub async fn update_record_with_executor<'e, E, M, PK, I>(
    executor: E,
    policy_pool: &sqlx::PgPool,
    descriptor: &'static ModelDescriptor<M, PK>,
    id: PK,
    input: I,
    ctx: &CoolContext,
    if_match: Option<i64>,
) -> Result<M, CoolError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    I: UpdateModelInput<M>,
    for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
    PK: Send + Clone + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
{
    input.validate()?;
    let values = input.sql_values();
    if values.is_empty() {
        return Err(CoolError::Validation(
            "update input must contain at least one changed column".to_owned(),
        ));
    }

    update_returning_record(
        executor,
        policy_pool,
        descriptor,
        id,
        &values,
        ctx,
        if_match,
    )
    .await
}

async fn insert_returning_record<'e, E, M, PK>(
    executor: E,
    descriptor: &'static ModelDescriptor<M, PK>,
    values: &[crate::SqlColumnValue],
) -> Result<M, CoolError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
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
        .fetch_one(executor)
        .await
        .map_err(|error| CoolError::Database(error.to_string()))
}

async fn update_returning_record<'e, E, M, PK>(
    executor: E,
    policy_pool: &sqlx::PgPool,
    descriptor: &'static ModelDescriptor<M, PK>,
    id: PK,
    values: &[crate::SqlColumnValue],
    ctx: &CoolContext,
    if_match: Option<i64>,
) -> Result<M, CoolError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow>,
    PK: Send + Clone + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
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
    let id_for_probe = id.clone();
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
        .fetch_optional(executor)
        .await
        .map_err(|error| CoolError::Database(error.to_string()))?;
    match outcome {
        Some(record) => Ok(record),
        None => {
            // No row matched. If this is a versioned update we want to
            // distinguish "stale version" from a true policy denial. The
            // probe applies the read policy: if the caller cannot see the
            // row, we keep returning Forbidden so policy denials remain
            // indistinguishable from missing rows.
            if let (Some(version_col), Some(expected)) = (version_column, if_match) {
                if let Some(current) =
                    probe_current_version(policy_pool, descriptor, id_for_probe, version_col, ctx)
                        .await?
                {
                    if current != expected {
                        return Err(CoolError::PreconditionFailed(format!(
                            "version mismatch: expected {expected}, found {current}",
                        )));
                    }
                }
            }
            Err(CoolError::Forbidden(
                "update policy denied this operation".to_owned(),
            ))
        }
    }
}

/// Read the current version of a row using the read policy. Returns `None` if
/// the caller cannot see the row (so the outer code preserves the existing
/// Forbidden-on-no-row semantics — readers can't tell a denied row from a
/// missing one).
async fn probe_current_version<M, PK>(
    policy_pool: &sqlx::PgPool,
    descriptor: &'static ModelDescriptor<M, PK>,
    id: PK,
    version_col: &'static str,
    ctx: &CoolContext,
) -> Result<Option<i64>, CoolError>
where
    PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
{
    let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("SELECT ");
    query.push(version_col);
    query.push(" FROM ").push(descriptor.table_name);
    query
        .push(" WHERE ")
        .push(descriptor.primary_key)
        .push(" = ");
    query.push_bind(id);
    query.push(" AND ");
    push_action_policy_query(
        &mut query,
        descriptor.read_allow_policies,
        descriptor.read_deny_policies,
        ctx,
    );

    let row: Option<(i64,)> = query
        .build_query_as::<(i64,)>()
        .fetch_optional(policy_pool)
        .await
        .map_err(|error| CoolError::Database(error.to_string()))?;
    Ok(row.map(|(v,)| v))
}

async fn delete_returning_record<'e, E, M, PK>(
    executor: E,
    descriptor: &'static ModelDescriptor<M, PK>,
    id: PK,
    ctx: &CoolContext,
) -> Result<M, CoolError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow>,
    PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
{
    let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("");
    match descriptor.soft_delete_column {
        Some(col) => {
            // Soft-delete: tombstone the row and bump version (if any) so
            // optimistic-lock semantics on subsequent updates stay coherent.
            query.push("UPDATE ").push(descriptor.table_name);
            query.push(" SET ").push(col).push(" = NOW()");
            if let Some(version_col) = descriptor.version_column {
                query
                    .push(", ")
                    .push(version_col)
                    .push(" = ")
                    .push(version_col)
                    .push(" + 1");
            }
            query.push(" WHERE ");
            query.push(col).push(" IS NULL AND ");
            query.push(descriptor.primary_key).push(" = ");
            query.push_bind(id);
        }
        None => {
            query.push("DELETE FROM ").push(descriptor.table_name);
            query.push(" WHERE ");
            query.push(descriptor.primary_key).push(" = ");
            query.push_bind(id);
        }
    }
    query.push(" AND ");
    push_action_policy_query(
        &mut query,
        descriptor.delete_allow_policies,
        descriptor.delete_deny_policies,
        ctx,
    );
    query
        .push(" RETURNING ")
        .push(descriptor.select_projection());

    query
        .build_query_as::<M>()
        .fetch_optional(executor)
        .await
        .map_err(|error| CoolError::Database(error.to_string()))?
        .ok_or_else(|| CoolError::Forbidden("delete policy denied this operation".to_owned()))
}

// ───── Upsert ──────────────────────────────────────────────────────────────
//
// `INSERT … ON CONFLICT (<pk>) DO UPDATE …`, but with the create/update
// distinction made *before* the SQL runs (via a `SELECT … FOR UPDATE` probe
// inside the same transaction) so we can:
//
//   * pick the right policy slot (both must allow at call time — see [docs])
//   * emit the correct ModelEventKind (Created vs Updated)
//   * capture an audit `before` snapshot only on the update branch
//
// The upsert is always transactional, regardless of whether the model emits
// events or has `@@audit`. That's a deliberate cost: one extra round-trip
// for the SELECT, in exchange for clean event/audit semantics. Upsert is
// not a hot read path — callers who need raw insert/update throughput
// should use `.create()` / `.update()` directly.

#[derive(Debug, Clone)]
pub struct UpsertRecord<'a, M: 'static, PK: 'static, I> {
    pub(crate) runtime: &'a SqlxRuntime,
    pub(crate) descriptor: &'static ModelDescriptor<M, PK>,
    pub(crate) input: I,
}

impl<'a, M: 'static, PK: 'static, I> UpsertRecord<'a, M, PK, I>
where
    I: UpsertModelInput<M>,
{
    /// Render an approximate SQL preview. The actual upsert wraps a
    /// `SELECT … FOR UPDATE` around the `INSERT … ON CONFLICT`, but this
    /// preview returns only the conflict-bearing statement — sufficient
    /// for migration tooling and the schema studio.
    pub fn preview_sql(&self) -> String {
        let values = self.input.sql_values();
        let placeholders = (1..=values.len())
            .map(|index| format!("${index}"))
            .collect::<Vec<_>>()
            .join(", ");
        let columns = values
            .iter()
            .map(|value| value.column)
            .collect::<Vec<_>>()
            .join(", ");
        let update_assignments = self
            .descriptor
            .upsert_update_columns
            .iter()
            .map(|column| format!("{column} = EXCLUDED.{column}"))
            .collect::<Vec<_>>()
            .join(", ");
        let version_bump = match self.descriptor.version_column {
            Some(col) => format!(", {col} = {table}.{col} + 1", table = self.descriptor.table_name, col = col),
            None => String::new(),
        };

        format!(
            "INSERT INTO {table} ({columns}) VALUES ({placeholders}) \
             ON CONFLICT ({pk}) DO UPDATE SET {update_assignments}{version_bump} \
             RETURNING {projection}",
            table = self.descriptor.table_name,
            pk = self.descriptor.primary_key,
            projection = self.descriptor.select_projection(),
        )
    }

    pub async fn run(self, ctx: &CoolContext) -> Result<M, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
        PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        let runtime = self.runtime;
        let mut tx = runtime
            .pool()
            .begin()
            .await
            .map_err(|error| CoolError::Database(error.to_string()))?;
        let (record, emits_event) = run_upsert_in_tx(
            &mut tx,
            runtime.pool(),
            self.descriptor,
            self.input,
            ctx,
        )
        .await?;
        tx.commit()
            .await
            .map_err(|error| CoolError::Database(error.to_string()))?;
        if emits_event {
            let _ = runtime.drain_event_outbox().await;
        }
        Ok(record)
    }

    /// Like [`Self::run`] but participates in a caller-supplied transaction.
    /// The upsert's `SELECT … FOR UPDATE` conflict probe runs against `tx`,
    /// not a fresh transaction — so the row lock is held until the caller
    /// commits. The event outbox is not drained here for the same reason
    /// as [`CreateRecord::run_in_tx`].
    pub async fn run_in_tx<'tx>(
        self,
        tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
        ctx: &CoolContext,
    ) -> Result<M, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
        PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        let (record, _) = run_upsert_in_tx(
            tx,
            self.runtime.pool(),
            self.descriptor,
            self.input,
            ctx,
        )
        .await?;
        Ok(record)
    }
}

/// Core upsert body: takes a live transaction and runs the conflict probe +
/// insert/update branch decision + audit + outbox enqueue. Returns the
/// resulting record and whether *any* model event was emitted (so the
/// owning `run()` can decide whether to drain the outbox post-commit).
async fn run_upsert_in_tx<'tx, M, PK, I>(
    tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
    policy_pool: &sqlx::PgPool,
    descriptor: &'static ModelDescriptor<M, PK>,
    input: I,
    ctx: &CoolContext,
) -> Result<(M, bool), CoolError>
where
    I: UpsertModelInput<M>,
    for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
    PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
{
    input.validate()?;

    // Compose the full insert value set, including auth-derived defaults
    // and the seeded `@version` column. Mirrors `create_record_with_executor`
    // so insert-branch semantics stay identical to `.create()`.
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

    // Both create and update policies must allow the call. Stricter than
    // "evaluate the path that runs," but it's the only choice we can make
    // before knowing which branch will fire — pre-flighting a read just to
    // pick the policy slot would leak row existence to the caller.
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
    let emits_created = descriptor.emits(ModelEventKind::Created);
    let emits_updated = descriptor.emits(ModelEventKind::Updated);
    let audit_enabled = descriptor.audit_enabled;

    if emits_created || emits_updated {
        ensure_event_outbox_table(&mut **tx).await?;
    }
    if audit_enabled {
        ensure_audit_table(policy_pool).await?;
    }

    // Probe the conflict target under a row-level lock. If a row exists,
    // this is the update branch and we capture the before-snapshot for
    // audit; otherwise it's the insert branch. The lock serializes
    // concurrent upserts on the same key, which is what callers expect.
    let before_record = select_for_update_by_pk_value(&mut **tx, descriptor, &pk_value).await?;
    let inserted = before_record.is_none();

    // For the update branch we additionally have to enforce the *update*
    // policy. The insert branch already passed `create` above; for the
    // update branch we evaluate the update policy against the live row
    // (using its current column values, not the input — that's how
    // ordinary `.update()` works) by re-running the policy SQL.
    if !inserted && !row_passes_update_policy(policy_pool, descriptor, &pk_value, ctx).await? {
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

    let record = upsert_returning_record(&mut **tx, descriptor, &insert_values).await?;

    // Event + audit fan-out, driven off whether the SELECT-FOR-UPDATE
    // saw a row. We don't lean on `xmax = 0`: keeping the discriminator
    // in the runtime (not the SQL) makes the rusqlite mirror trivial.
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
    let emits_event = if inserted { emits_created } else { emits_updated };

    if emits_event {
        enqueue_event_outbox(&mut **tx, descriptor.schema_name, event_kind, &record).await?;
    }
    if audit_enabled {
        let after = serde_json::to_value(&record).ok();
        let event = build_audit_event(descriptor, audit_op, before_snapshot, after, ctx);
        enqueue_audit_event(&mut **tx, &event).await?;
    }

    Ok((record, emits_event))
}

/// Probe-with-lock: `SELECT projection FROM <table> WHERE <pk> = $1 FOR UPDATE`.
/// Bypasses read policies — we need the raw row to drive insert/update
/// branching and to capture the audit before-snapshot. Returns `None` when
/// no row exists (the insert branch).
async fn select_for_update_by_pk_value<'e, E, M, PK>(
    executor: E,
    descriptor: &'static ModelDescriptor<M, PK>,
    pk_value: &SqlValue,
) -> Result<Option<M>, CoolError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
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
    // Soft-deleted rows act as "no row" for upsert purposes: the INSERT
    // branch will then fail on the PK uniqueness constraint, which is the
    // right outcome (refuse to silently revive a tombstone).
    if let Some(col) = descriptor.soft_delete_column {
        query.push(" AND ").push(col).push(" IS NULL");
    }
    query.push(" FOR UPDATE");

    query
        .build_query_as::<M>()
        .fetch_optional(executor)
        .await
        .map_err(|error| CoolError::Database(error.to_string()))
}

/// Re-evaluate the update policy against an existing row, using the read
/// pool so the policy predicates can resolve auth/tenancy. Returns `false`
/// when the policy denies (or when the row is not visible to the caller,
/// which we treat as denial — same semantics as ordinary `.update()`).
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

/// Render and execute the conflict-bearing INSERT. The DO UPDATE clause
/// references only columns in `descriptor.upsert_update_columns` — PK,
/// `@version`, `@readonly`, `@server_only`, and `@default(...)` columns are
/// excluded by construction (see `generate_model_descriptor`).
async fn upsert_returning_record<'e, E, M, PK>(
    executor: E,
    descriptor: &'static ModelDescriptor<M, PK>,
    insert_values: &[crate::SqlColumnValue],
) -> Result<M, CoolError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
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
    query.push(") ON CONFLICT (").push(descriptor.primary_key).push(") DO UPDATE SET ");

    // The DO UPDATE list. If there are no eligible columns to overwrite,
    // fall back to "DO NOTHING"-equivalent semantics via a no-op assignment:
    // touching the PK to itself. This keeps the RETURNING clause working
    // (PG only RETURNs from rows the statement touched), which matters for
    // round-trips that always want the current row back.
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
        .fetch_one(executor)
        .await
        .map_err(|error| CoolError::Database(error.to_string()))
}

// ───── UpdateMany ──────────────────────────────────────────────────────────
//
// Bulk UPDATE-by-predicate: emit a single statement that mutates every row
// the filter matches AND the update policy admits, in one round-trip.
//
// Differences from per-row `.update(id).set(input)`:
//   * No `if_match` slot. Bulk updates aren't an optimistic-locking idiom;
//     when a model has `@version`, the column is auto-incremented for every
//     matched row (mirroring `batch_update`'s rendering) and the caller does
//     NOT supply an expected version. If you need per-row CAS, use
//     `batch_update(...)` or `.update(id).set(...).if_match(...)`.
//   * Requires at least one filter. A predicate-less bulk update is almost
//     always a footgun (table-wide rewrite, event storm); callers who truly
//     want that should write raw SQL so the intent is obvious in review.
//
// Return value is `BatchSummary { total, ok, err }` where `total = ok =
// rows actually updated` and `err = 0` — the WHERE either matched a row
// or it didn't, there's no per-item rejection vocabulary to populate.

#[derive(Debug, Clone)]
pub struct UpdateMany<'a, M: 'static, PK: 'static> {
    pub(crate) runtime: &'a SqlxRuntime,
    pub(crate) descriptor: &'static ModelDescriptor<M, PK>,
    pub(crate) filters: Vec<FilterExpr>,
}

impl<'a, M: 'static, PK: 'static> UpdateMany<'a, M, PK> {
    pub fn where_(mut self, filter: crate::Filter) -> Self {
        self.filters.push(FilterExpr::from(filter));
        self
    }

    pub fn where_expr(mut self, filter: FilterExpr) -> Self {
        self.filters.push(filter);
        self
    }

    pub fn where_any(mut self, filters: impl IntoIterator<Item = FilterExpr>) -> Self {
        self.filters.push(FilterExpr::any(filters));
        self
    }

    /// Supply the patch values. Returns a builder ready to `.run(ctx)`.
    pub fn set<I>(self, input: I) -> UpdateManySet<'a, M, PK, I> {
        UpdateManySet {
            runtime: self.runtime,
            descriptor: self.descriptor,
            filters: self.filters,
            input,
        }
    }
}

#[derive(Debug, Clone)]
pub struct UpdateManySet<'a, M: 'static, PK: 'static, I> {
    pub(crate) runtime: &'a SqlxRuntime,
    pub(crate) descriptor: &'static ModelDescriptor<M, PK>,
    pub(crate) filters: Vec<FilterExpr>,
    pub(crate) input: I,
}

impl<'a, M: 'static, PK: 'static, I> UpdateManySet<'a, M, PK, I>
where
    I: UpdateModelInput<M>,
{
    /// Approximate SQL preview. The actual query interpolates filter
    /// predicates and the update policy clause; this returns a sketch
    /// good enough for migration tooling and the schema studio.
    pub fn preview_sql(&self) -> String {
        let values = self.input.sql_values();
        let columns: Vec<&str> = values.iter().map(|v| v.column).collect();
        render_update_many_preview_sql(
            self.descriptor.table_name,
            self.descriptor.soft_delete_column.is_some(),
            self.descriptor.version_column,
            &columns,
            &self.descriptor.select_projection(),
        )
    }

    /// Run the bulk update. Returns a `BatchSummary` where `total = ok =
    /// rows_affected` and `err = 0`. Statement-level failures (DB error,
    /// validation, empty filter list) surface as the outer `Err`.
    pub async fn run(self, ctx: &CoolContext) -> Result<BatchSummary, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
    {
        let runtime = self.runtime;
        let descriptor = self.descriptor;
        let mut tx = runtime
            .pool()
            .begin()
            .await
            .map_err(|error| CoolError::Database(error.to_string()))?;
        let (summary, emits_event) =
            run_update_many_in_tx(&mut tx, runtime.pool(), descriptor, &self.filters, self.input, ctx).await?;
        tx.commit()
            .await
            .map_err(|error| CoolError::Database(error.to_string()))?;
        if emits_event {
            let _ = runtime.drain_event_outbox().await;
        }
        Ok(summary)
    }

    /// Run inside a caller-supplied transaction. Audit + outbox writes
    /// land in `tx` alongside the bulk UPDATE; the caller commits and is
    /// responsible for draining the event outbox afterwards if they want
    /// immediate fan-out.
    pub async fn run_in_tx<'tx>(
        self,
        tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
        ctx: &CoolContext,
    ) -> Result<BatchSummary, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
    {
        let (summary, _) =
            run_update_many_in_tx(tx, self.runtime.pool(), self.descriptor, &self.filters, self.input, ctx).await?;
        Ok(summary)
    }
}

async fn run_update_many_in_tx<'tx, M, PK, I>(
    tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
    policy_pool: &sqlx::PgPool,
    descriptor: &'static ModelDescriptor<M, PK>,
    filters: &[FilterExpr],
    input: I,
    ctx: &CoolContext,
) -> Result<(BatchSummary, bool), CoolError>
where
    I: UpdateModelInput<M>,
    for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
{
    if filters.is_empty() {
        return Err(CoolError::Validation(
            "update_many requires at least one filter — refusing table-wide update".to_owned(),
        ));
    }
    input.validate()?;
    let values = input.sql_values();
    if values.is_empty() {
        return Err(CoolError::Validation(
            "update input must contain at least one changed column".to_owned(),
        ));
    }

    let emits_event = descriptor.emits(ModelEventKind::Updated);
    let audit_enabled = descriptor.audit_enabled;
    if emits_event {
        ensure_event_outbox_table(&mut **tx).await?;
    }
    if audit_enabled {
        ensure_audit_table(policy_pool).await?;
    }

    // Build the bulk UPDATE with RETURNING — we always read back the
    // mutated rows so audit/event fan-out works and `BatchSummary.ok` is
    // accurate. Even when neither is enabled, the RETURNING cost is a
    // single round-trip we accept for the simpler control flow.
    let version_column = descriptor.version_column;
    let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("UPDATE ");
    query.push(descriptor.table_name).push(" SET ");
    for (idx, value) in values.iter().enumerate() {
        if idx > 0 {
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

    query.push(" WHERE ");
    let mut wrote = false;
    if let Some(col) = descriptor.soft_delete_column {
        query.push(col).push(" IS NULL");
        wrote = true;
    }
    if wrote {
        query.push(" AND ");
    }
    query.push("(");
    push_filter_query(&mut query, filters);
    query.push(") AND ");
    push_action_policy_query(
        &mut query,
        descriptor.update_allow_policies,
        descriptor.update_deny_policies,
        ctx,
    );
    query
        .push(" RETURNING ")
        .push(descriptor.select_projection());

    let updated: Vec<M> = query
        .build_query_as::<M>()
        .fetch_all(&mut **tx)
        .await
        .map_err(|error| CoolError::Database(error.to_string()))?;

    for record in &updated {
        if emits_event {
            enqueue_event_outbox(
                &mut **tx,
                descriptor.schema_name,
                ModelEventKind::Updated,
                record,
            )
            .await?;
        }
        if audit_enabled {
            // No before-snapshot: capturing one would require a SELECT
            // FOR UPDATE of every matched row before the UPDATE, doubling
            // the round-trips. The audit row records the after state and
            // the operation kind; consumers wanting a diff compare against
            // the previous audit row for the same PK.
            let after = serde_json::to_value(record).ok();
            let event =
                build_audit_event(descriptor, AuditOperation::Update, None, after, ctx);
            enqueue_audit_event(&mut **tx, &event).await?;
        }
    }

    let total = updated.len();
    Ok((
        BatchSummary {
            total,
            ok: total,
            err: 0,
        },
        emits_event,
    ))
}
