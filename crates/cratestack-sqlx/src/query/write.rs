use cratestack_core::{AuditOperation, CoolContext, CoolError, ModelEventKind};

use crate::{
    CreateModelInput, ModelDescriptor, SqlxRuntime, UpdateModelInput,
    audit::{build_audit_event, enqueue_audit_event, ensure_audit_table, fetch_for_audit},
    descriptor::{enqueue_event_outbox, ensure_event_outbox_table},
};

use super::support::{
    apply_create_defaults, evaluate_create_policies, find_column_value, push_action_policy_query,
    push_bind_value,
};

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
