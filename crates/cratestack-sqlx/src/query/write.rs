use cratestack_core::{CoolContext, CoolError, ModelEventKind};

use crate::{
    CreateModelInput, ModelDescriptor, SqlValue, SqlxRuntime, UpdateModelInput,
    descriptor::{enqueue_event_outbox, ensure_event_outbox_table},
};

use super::support::{
    apply_create_defaults, push_action_policy_query, push_bind_value,
};

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
        let mut tx = self
            .runtime
            .pool()
            .begin()
            .await
            .map_err(|error| CoolError::Database(error.to_string()))?;
        if emits_event {
            ensure_event_outbox_table(&mut *tx).await?;
        }
        let record =
            create_record_with_executor(&mut *tx, self.descriptor, self.input, ctx).await?;
        if emits_event {
            enqueue_event_outbox(
                &mut *tx,
                self.descriptor.schema_name,
                ModelEventKind::Created,
                &record,
            )
            .await?;
        }
        tx.commit()
            .await
            .map_err(|error| CoolError::Database(error.to_string()))?;

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
        }
    }
}

#[derive(Debug, Clone)]
pub struct UpdateRecordSet<'a, M: 'static, PK: 'static, I> {
    pub(crate) runtime: &'a SqlxRuntime,
    pub(crate) descriptor: &'static ModelDescriptor<M, PK>,
    pub(crate) id: PK,
    pub(crate) input: I,
}

impl<'a, M: 'static, PK: 'static, I> UpdateRecordSet<'a, M, PK, I>
where
    I: UpdateModelInput<M>,
{
    pub fn preview_sql(&self) -> String {
        let values = self.input.sql_values();
        let assignments = values
            .iter()
            .enumerate()
            .map(|(index, value)| format!("{} = ${}", value.column, index + 1))
            .collect::<Vec<_>>()
            .join(", ");

        format!(
            "UPDATE {} SET {} WHERE {} = ${} RETURNING {}",
            self.descriptor.table_name,
            assignments,
            self.descriptor.primary_key,
            values.len() + 1,
            self.descriptor.select_projection(),
        )
    }

    pub async fn run(self, ctx: &CoolContext) -> Result<M, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
        PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        let emits_event = self.descriptor.emits(ModelEventKind::Updated);
        let record = if emits_event {
            let mut tx = self
                .runtime
                .pool()
                .begin()
                .await
                .map_err(|error| CoolError::Database(error.to_string()))?;
            ensure_event_outbox_table(&mut *tx).await?;
            let record =
                update_record_with_executor(&mut *tx, self.descriptor, self.id, self.input, ctx)
                    .await?;
            enqueue_event_outbox(
                &mut *tx,
                self.descriptor.schema_name,
                ModelEventKind::Updated,
                &record,
            )
            .await?;
            tx.commit()
                .await
                .map_err(|error| CoolError::Database(error.to_string()))?;
            record
        } else {
            update_record_with_executor(
                self.runtime.pool(),
                self.descriptor,
                self.id,
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
        let record = if emits_event {
            let mut tx = self
                .runtime
                .pool()
                .begin()
                .await
                .map_err(|error| CoolError::Database(error.to_string()))?;
            ensure_event_outbox_table(&mut *tx).await?;

            let record = delete_returning_record(&mut *tx, self.descriptor, self.id, ctx).await?;
            enqueue_event_outbox(
                &mut *tx,
                self.descriptor.schema_name,
                ModelEventKind::Deleted,
                &record,
            )
            .await?;
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

pub async fn create_record_with_executor<M, PK, I>(
    connection: &mut sqlx::PgConnection,
    descriptor: &'static ModelDescriptor<M, PK>,
    input: I,
    ctx: &CoolContext,
) -> Result<M, CoolError>
where
    I: CreateModelInput<M>,
    for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
{
    let values = apply_create_defaults(input.sql_values(), descriptor.create_defaults, ctx)?;
    if values.is_empty() {
        return Err(CoolError::Validation(
            "create input must contain at least one column".to_owned(),
        ));
    }
    let record = insert_returning_record(&mut *connection, descriptor, &values).await?;
    authorize_created_record_with_executor(&mut *connection, descriptor, &record, ctx).await?;
    Ok(record)
}

pub async fn update_record_with_executor<'e, E, M, PK, I>(
    executor: E,
    descriptor: &'static ModelDescriptor<M, PK>,
    id: PK,
    input: I,
    ctx: &CoolContext,
) -> Result<M, CoolError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    I: UpdateModelInput<M>,
    for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
    PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
{
    let values = input.sql_values();
    if values.is_empty() {
        return Err(CoolError::Validation(
            "update input must contain at least one changed column".to_owned(),
        ));
    }

    update_returning_record(executor, descriptor, id, &values, ctx).await
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

async fn authorize_created_record_with_executor<'e, E, M, PK>(
    executor: E,
    descriptor: &'static ModelDescriptor<M, PK>,
    record: &M,
    ctx: &CoolContext,
) -> Result<(), CoolError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    M: serde::Serialize,
{
    let id = created_record_primary_key(descriptor, record)?;
    let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("SELECT 1 FROM ");
    query
        .push(descriptor.table_name)
        .push(" WHERE ")
        .push(descriptor.primary_key)
        .push(" = ");
    push_bind_value(&mut query, &id);
    query.push(" AND ");
    push_action_policy_query(
        &mut query,
        descriptor.create_allow_policies,
        descriptor.create_deny_policies,
        ctx,
    );
    query.push(" LIMIT 1");

    let authorized = query
        .build_query_scalar::<i32>()
        .fetch_optional(executor)
        .await
        .map_err(|error| CoolError::Database(error.to_string()))?
        .is_some();

    if authorized {
        Ok(())
    } else {
        Err(CoolError::Forbidden(
            "create policy denied this operation".to_owned(),
        ))
    }
}

fn created_record_primary_key<M, PK>(
    descriptor: &'static ModelDescriptor<M, PK>,
    record: &M,
) -> Result<SqlValue, CoolError>
where
    M: serde::Serialize,
{
    let rust_field = descriptor
        .columns
        .iter()
        .find(|column| column.sql_name == descriptor.primary_key)
        .map(|column| column.rust_name)
        .ok_or_else(|| {
            CoolError::Validation(format!(
                "model descriptor `{}` is missing primary key column `{}`",
                descriptor.schema_name, descriptor.primary_key
            ))
        })?;

    let serde_json::Value::Object(fields) =
        serde_json::to_value(record).map_err(|error| CoolError::Validation(error.to_string()))?
    else {
        return Err(CoolError::Validation(format!(
            "created `{}` record did not serialize to an object",
            descriptor.schema_name
        )));
    };

    let value = fields.get(rust_field).cloned().ok_or_else(|| {
        CoolError::Validation(format!(
            "created `{}` record is missing primary key field `{}`",
            descriptor.schema_name, rust_field
        ))
    })?;

    sql_value_from_json(value).ok_or_else(|| {
        CoolError::Validation(format!(
            "failed to convert primary key field `{}` on created `{}` record into a SQL bind value",
            rust_field, descriptor.schema_name
        ))
    })
}

fn sql_value_from_json(value: serde_json::Value) -> Option<SqlValue> {
    match value {
        serde_json::Value::Bool(value) => Some(SqlValue::Bool(value)),
        serde_json::Value::Number(value) => value.as_i64().map(SqlValue::Int),
        serde_json::Value::String(value) => Some(SqlValue::String(value)),
        _ => None,
    }
}

async fn update_returning_record<'e, E, M, PK>(
    executor: E,
    descriptor: &'static ModelDescriptor<M, PK>,
    id: PK,
    values: &[crate::SqlColumnValue],
    ctx: &CoolContext,
) -> Result<M, CoolError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow>,
    PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
{
    let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("UPDATE ");
    query.push(descriptor.table_name).push(" SET ");
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            query.push(", ");
        }
        query.push(value.column).push(" = ");
        push_bind_value(&mut query, &value.value);
    }
    query
        .push(" WHERE ")
        .push(descriptor.primary_key)
        .push(" = ");
    query.push_bind(id);
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

    query
        .build_query_as::<M>()
        .fetch_optional(executor)
        .await
        .map_err(|error| CoolError::Database(error.to_string()))?
        .ok_or_else(|| CoolError::Forbidden("update policy denied this operation".to_owned()))
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
    let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("DELETE FROM ");
    query.push(descriptor.table_name).push(" WHERE ");
    query.push(descriptor.primary_key).push(" = ");
    query.push_bind(id);
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
