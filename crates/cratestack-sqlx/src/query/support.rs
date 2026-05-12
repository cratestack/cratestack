use cratestack_core::{CoolContext, CoolError, Value};
use cratestack_policy::{context_has_role, context_in_tenant, value_matches_policy_literal};

use cratestack_sql::{
    policy_render::{auth_value_to_sql as policy_auth_value_to_sql, render_action_policy},
    render::{render_filter_exprs, render_order_clause, SqlSink},
    OrderClause,
};

use crate::{
    CreateDefault, CreateDefaultType, FilterExpr, ModelDescriptor, PolicyExpr, PolicyLiteral,
    ReadPolicy, ReadPredicate, RelationQuantifier, SqlColumnValue, SqlValue, SqlxRuntime,
};

/// `SqlSink` adapter that writes into a `sqlx::QueryBuilder`. Literal SQL
/// goes through `push`; binds go through `push_bind` (which emits the
/// placeholder automatically). This is what turns the shared renderer into
/// the run-time query path.
pub(crate) struct QueryBuilderSink<'a, 'qb> {
    qb: &'a mut sqlx::QueryBuilder<'qb, sqlx::Postgres>,
}

impl<'a, 'qb> QueryBuilderSink<'a, 'qb> {
    pub(crate) fn new(qb: &'a mut sqlx::QueryBuilder<'qb, sqlx::Postgres>) -> Self {
        Self { qb }
    }
}

impl<'a, 'qb> SqlSink for QueryBuilderSink<'a, 'qb> {
    fn push_sql(&mut self, sql: &str) {
        self.qb.push(sql);
    }

    fn push_bind(&mut self, value: &SqlValue) {
        push_bind_value(self.qb, value);
    }
}

pub(crate) fn push_scoped_conditions<'a, M, PK, Id>(
    query: &mut sqlx::QueryBuilder<'a, sqlx::Postgres>,
    descriptor: &ModelDescriptor<M, PK>,
    filters: &[FilterExpr],
    primary_key: Option<(&'static str, Id)>,
    ctx: &CoolContext,
) where
    Id: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres> + 'a,
{
    query.push(" WHERE ");

    let mut wrote_clause = false;
    // Soft-delete filter: hide tombstoned rows from every read. Banks treat
    // the audit log as the source of truth for what changed; this just
    // prevents deleted rows from leaking back into list/get responses.
    if let Some(col) = descriptor.lifecycle.soft_delete_column {
        query.push(col).push(" IS NULL");
        wrote_clause = true;
    }
    if !filters.is_empty() {
        if wrote_clause {
            query.push(" AND ");
        }
        push_filter_query(query, filters);
        wrote_clause = true;
    }

    if let Some((primary_key, id)) = primary_key {
        if wrote_clause {
            query.push(" AND ");
        }
        query.push(primary_key).push(" = ");
        query.push_bind(id);
        wrote_clause = true;
    }

    if wrote_clause {
        query.push(" AND ");
    }
    push_action_policy_query(
        query,
        descriptor.auth.read_allow_policies,
        descriptor.auth.read_deny_policies,
        ctx,
    );
}

pub(crate) fn push_filter_query(
    query: &mut sqlx::QueryBuilder<'_, sqlx::Postgres>,
    filters: &[FilterExpr],
) {
    let mut sink = QueryBuilderSink::new(query);
    render_filter_exprs(&mut sink, filters);
}

pub(crate) fn push_action_policy_query(
    query: &mut sqlx::QueryBuilder<'_, sqlx::Postgres>,
    allow_policies: &[ReadPolicy],
    deny_policies: &[ReadPolicy],
    ctx: &CoolContext,
) {
    let mut sink = QueryBuilderSink::new(query);
    render_action_policy(&mut sink, allow_policies, deny_policies, ctx);
}

pub(crate) fn push_policy_expr_query(
    query: &mut sqlx::QueryBuilder<'_, sqlx::Postgres>,
    expr: PolicyExpr,
    ctx: &CoolContext,
) {
    let mut sink = QueryBuilderSink::new(query);
    cratestack_sql::policy_render::render_policy_expr(&mut sink, expr, ctx);
}

pub(crate) fn push_order_and_paging(
    query: &mut sqlx::QueryBuilder<'_, sqlx::Postgres>,
    order_by: &[OrderClause],
    limit: Option<i64>,
    offset: Option<i64>,
) {
    if !order_by.is_empty() {
        query.push(" ORDER BY ");
        for (index, clause) in order_by.iter().enumerate() {
            if index > 0 {
                query.push(", ");
            }
            let mut sink = QueryBuilderSink::new(query);
            render_order_clause(&mut sink, clause);
        }
    }

    match (limit, offset) {
        (Some(limit), Some(offset)) => {
            query.push(" LIMIT ");
            query.push_bind(limit);
            query.push(" OFFSET ");
            query.push_bind(offset);
        }
        (Some(limit), None) => {
            query.push(" LIMIT ");
            query.push_bind(limit);
        }
        (None, Some(offset)) => {
            query.push(" OFFSET ");
            query.push_bind(offset);
        }
        (None, None) => {}
    }
}

/// Convert a context auth value into a `SqlValue` suitable for binding.
/// Crate-local re-export of the shared helper so existing callers don't need
/// to import a new module.
pub(crate) fn auth_value_to_sql(ctx: &CoolContext, auth_field: &str) -> Option<SqlValue> {
    policy_auth_value_to_sql(ctx, auth_field)
}


pub(crate) async fn authorize_record_action<M, PK>(
    runtime: &SqlxRuntime,
    descriptor: &'static ModelDescriptor<M, PK>,
    id: PK,
    allow_policies: &[ReadPolicy],
    deny_policies: &[ReadPolicy],
    ctx: &CoolContext,
    action_name: &str,
) -> Result<(), CoolError>
where
    PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
{
    let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("SELECT 1 FROM ");
    query
        .push(descriptor.table.table_name)
        .push(" WHERE ")
        .push(descriptor.table.primary_key)
        .push(" = ");
    query.push_bind(id);
    query.push(" AND ");
    push_action_policy_query(&mut query, allow_policies, deny_policies, ctx);
    query.push(" LIMIT 1");

    let authorized = query
        .build_query_scalar::<i32>()
        .fetch_optional(runtime.pool())
        .await
        .map_err(|error| CoolError::Database(error.to_string()))?
        .is_some();

    if authorized {
        Ok(())
    } else {
        Err(CoolError::Forbidden(format!(
            "{action_name} policy denied this operation"
        )))
    }
}

pub(crate) async fn evaluate_create_policies(
    pool: &sqlx::PgPool,
    allow_policies: &[ReadPolicy],
    deny_policies: &[ReadPolicy],
    values: &[SqlColumnValue],
    ctx: &CoolContext,
) -> Result<bool, CoolError> {
    if allow_policies.is_empty() {
        return Ok(false);
    }

    for policy in deny_policies {
        if evaluate_create_policy_expr(pool, policy.expr, values, ctx).await? {
            return Ok(false);
        }
    }

    for policy in allow_policies {
        if evaluate_create_policy_expr(pool, policy.expr, values, ctx).await? {
            return Ok(true);
        }
    }

    Ok(false)
}

pub(crate) fn apply_create_defaults(
    mut values: Vec<SqlColumnValue>,
    defaults: &[CreateDefault],
    ctx: &CoolContext,
) -> Result<Vec<SqlColumnValue>, CoolError> {
    for default in defaults {
        if find_column_value(&values, default.column).is_some() {
            continue;
        }
        let value = match (
            ctx.auth_field(default.auth_field),
            default.ty,
            default.nullable,
        ) {
            (Some(Value::Bool(value)), CreateDefaultType::Bool, _) => SqlValue::Bool(*value),
            (Some(Value::Int(value)), CreateDefaultType::Int, _) => SqlValue::Int(*value),
            (Some(Value::String(value)), CreateDefaultType::String, _) => {
                SqlValue::String(value.clone())
            }
            (None, CreateDefaultType::Bool, true) => SqlValue::NullBool,
            (None, CreateDefaultType::Int, true) => SqlValue::NullInt,
            (None, CreateDefaultType::String, true) => SqlValue::NullString,
            (None, _, false) if !ctx.is_authenticated() => {
                return Err(CoolError::Forbidden(
                    "create policy denied this operation".to_owned(),
                ));
            }
            (None, _, false) => {
                return Err(CoolError::Validation(format!(
                    "missing auth field `{}` required for create default on `{}`",
                    default.auth_field, default.column
                )));
            }
            (Some(_), _, _) => {
                return Err(CoolError::Validation(format!(
                    "auth field `{}` has incompatible type for create default on `{}`",
                    default.auth_field, default.column
                )));
            }
        };
        values.push(SqlColumnValue {
            column: default.column,
            value,
        });
    }

    Ok(values)
}

pub(crate) fn find_column_value<'a>(
    values: &'a [SqlColumnValue],
    column: &str,
) -> Option<&'a SqlValue> {
    values
        .iter()
        .find(|value| value.column == column)
        .map(|value| &value.value)
}

pub(crate) fn sql_value_matches_literal(value: &SqlValue, literal: PolicyLiteral) -> bool {
    match (value, literal) {
        (SqlValue::Bool(left), PolicyLiteral::Bool(right)) => *left == right,
        (SqlValue::Int(left), PolicyLiteral::Int(right)) => *left == right,
        (SqlValue::String(left), PolicyLiteral::String(right)) => left == right,
        _ => false,
    }
}

pub(crate) fn push_bind_value(
    query: &mut sqlx::QueryBuilder<'_, sqlx::Postgres>,
    value: &SqlValue,
) {
    match value {
        SqlValue::Bool(value) => query.push_bind(*value),
        SqlValue::Int(value) => query.push_bind(*value),
        SqlValue::Float(value) => query.push_bind(*value),
        SqlValue::String(value) => query.push_bind(value.clone()),
        SqlValue::Bytes(value) => query.push_bind(value.clone()),
        SqlValue::Uuid(value) => query.push_bind(*value),
        SqlValue::DateTime(value) => query.push_bind(*value),
        SqlValue::Json(value) => query.push_bind(sqlx::types::Json(value.clone())),
        SqlValue::Decimal(value) => query.push_bind(*value),
        SqlValue::NullBool => query.push_bind(Option::<bool>::None),
        SqlValue::NullInt => query.push_bind(Option::<i64>::None),
        SqlValue::NullFloat => query.push_bind(Option::<f64>::None),
        SqlValue::NullString => query.push_bind(Option::<String>::None),
        SqlValue::NullBytes => query.push_bind(Option::<Vec<u8>>::None),
        SqlValue::NullUuid => query.push_bind(Option::<uuid::Uuid>::None),
        SqlValue::NullDateTime => query.push_bind(Option::<chrono::DateTime<chrono::Utc>>::None),
        SqlValue::NullJson => query.push_bind(Option::<sqlx::types::Json<Value>>::None),
        SqlValue::NullDecimal => query.push_bind(Option::<cratestack_core::Decimal>::None),
    };
}

fn evaluate_input_predicate(
    predicate: ReadPredicate,
    values: &[SqlColumnValue],
    ctx: &CoolContext,
) -> bool {
    match predicate {
        ReadPredicate::AuthNotNull => ctx.is_authenticated(),
        ReadPredicate::AuthIsNull => !ctx.is_authenticated(),
        ReadPredicate::HasRole { role } => context_has_role(ctx, role),
        ReadPredicate::InTenant { tenant_id } => context_in_tenant(ctx, tenant_id),
        ReadPredicate::AuthFieldEqLiteral { auth_field, value } => ctx
            .auth_field(auth_field)
            .is_some_and(|candidate| value_matches_policy_literal(candidate, value)),
        ReadPredicate::AuthFieldNeLiteral { auth_field, value } => ctx
            .auth_field(auth_field)
            .is_some_and(|candidate| !value_matches_policy_literal(candidate, value)),
        ReadPredicate::FieldIsTrue { column } => {
            find_column_value(values, column) == Some(&SqlValue::Bool(true))
        }
        ReadPredicate::FieldEqLiteral { column, value } => find_column_value(values, column)
            .is_some_and(|candidate| sql_value_matches_literal(candidate, value)),
        ReadPredicate::FieldNeLiteral { column, value } => find_column_value(values, column)
            .is_some_and(|candidate| !sql_value_matches_literal(candidate, value)),
        ReadPredicate::FieldEqAuth { column, auth_field } => {
            match (
                find_column_value(values, column),
                auth_value_to_sql(ctx, auth_field),
            ) {
                (Some(candidate), Some(auth_value)) => candidate == &auth_value,
                _ => false,
            }
        }
        ReadPredicate::FieldNeAuth { column, auth_field } => {
            match (
                find_column_value(values, column),
                auth_value_to_sql(ctx, auth_field),
            ) {
                (Some(candidate), Some(auth_value)) => candidate != &auth_value,
                _ => false,
            }
        }
        ReadPredicate::Relation { .. } => false,
    }
}

fn evaluate_create_policy_expr<'a>(
    pool: &'a sqlx::PgPool,
    expr: PolicyExpr,
    values: &'a [SqlColumnValue],
    ctx: &'a CoolContext,
) -> core::pin::Pin<Box<dyn core::future::Future<Output = Result<bool, CoolError>> + Send + 'a>> {
    Box::pin(async move {
        match expr {
            PolicyExpr::Predicate(predicate) => {
                evaluate_create_predicate(pool, predicate, values, ctx).await
            }
            PolicyExpr::And(exprs) => {
                for expr in exprs.iter().copied() {
                    if !evaluate_create_policy_expr(pool, expr, values, ctx).await? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            PolicyExpr::Or(exprs) => {
                for expr in exprs.iter().copied() {
                    if evaluate_create_policy_expr(pool, expr, values, ctx).await? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
        }
    })
}

fn evaluate_create_predicate<'a>(
    pool: &'a sqlx::PgPool,
    predicate: ReadPredicate,
    values: &'a [SqlColumnValue],
    ctx: &'a CoolContext,
) -> core::pin::Pin<Box<dyn core::future::Future<Output = Result<bool, CoolError>> + Send + 'a>> {
    Box::pin(async move {
        match predicate {
            ReadPredicate::Relation {
                quantifier,
                parent_column,
                related_table,
                related_column,
                expr,
                ..
            } => {
                let Some(parent_value) = find_column_value(values, parent_column) else {
                    return Ok(false);
                };

                let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("SELECT ");
                let opening = match quantifier {
                    RelationQuantifier::ToOne | RelationQuantifier::Some => {
                        "EXISTS (SELECT 1 FROM "
                    }
                    RelationQuantifier::None | RelationQuantifier::Every => {
                        "NOT EXISTS (SELECT 1 FROM "
                    }
                };
                query.push(opening);
                query.push(related_table);
                query.push(" WHERE ");
                query.push(related_table);
                query.push(".");
                query.push(related_column);
                query.push(" = ");
                push_bind_value(&mut query, parent_value);
                match quantifier {
                    RelationQuantifier::Every => query.push(" AND NOT ("),
                    _ => query.push(" AND "),
                };
                push_policy_expr_query(&mut query, *expr, ctx);
                match quantifier {
                    RelationQuantifier::Every => query.push("))"),
                    _ => query.push(")"),
                };

                let result: (bool,) = query
                    .build_query_as::<(bool,)>()
                    .fetch_one(pool)
                    .await
                    .map_err(|error| CoolError::Database(error.to_string()))?;
                Ok(result.0)
            }
            _ => Ok(evaluate_input_predicate(predicate, values, ctx)),
        }
    })
}
