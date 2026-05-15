use crate::sqlx;

use cratestack_core::{CoolContext, CoolError, Value};
use cratestack_policy::{context_has_role, context_in_tenant};

use cratestack_sql::{FilterOp, FilterValue, OrderTarget};

use crate::{
    CreateDefault, CreateDefaultType, FilterExpr, ModelDescriptor, OrderClause, PolicyExpr,
    PolicyLiteral, ReadPolicy, ReadPredicate, RelationFilter, RelationQuantifier, SortDirection,
    SqlColumnValue, SqlValue, SqlxRuntime,
};

/// Which policy slot to consult when filtering rows from a read query.
/// Schemas can declare separate `@@allow("list", ...)` (folded into
/// `read_*`) and `@@allow("detail", ...)` (folded into `detail_*`)
/// predicates; the right slot depends on what kind of read is happening.
/// Bulk and listing operations apply List; single-row lookups (where the
/// caller is asking for a specific row by PK or unique key) apply
/// Detail. The toggle is exposed on `FindUnique` via `.as_detail()` /
/// `.as_list()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReadPolicyKind {
    /// `read_*` policies — used by list-style reads (`find_many`,
    /// `batch_get`, scoped updates/deletes that filter by PK).
    List,
    /// `detail_*` policies — used by single-row lookups
    /// (`find_unique`). Falls back to the list policies when the schema
    /// hasn't declared explicit detail rules.
    Detail,
}

pub(crate) fn push_scoped_conditions<'a, M, PK, Id>(
    query: &mut sqlx::QueryBuilder<'a, sqlx::Postgres>,
    descriptor: &ModelDescriptor<M, PK>,
    filters: &[FilterExpr],
    primary_key: Option<(&'static str, Id)>,
    ctx: &CoolContext,
    policy_kind: ReadPolicyKind,
) where
    Id: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres> + 'a,
{
    query.push(" WHERE ");

    let mut wrote_clause = false;
    // Soft-delete filter: hide tombstoned rows from every read. Banks treat
    // the audit log as the source of truth for what changed; this just
    // prevents deleted rows from leaking back into list/get responses.
    if let Some(col) = descriptor.soft_delete_column {
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
    let (allow, deny) = match policy_kind {
        ReadPolicyKind::List => (descriptor.read_allow_policies, descriptor.read_deny_policies),
        ReadPolicyKind::Detail => (
            descriptor.detail_allow_policies,
            descriptor.detail_deny_policies,
        ),
    };
    push_action_policy_query(query, allow, deny, ctx);
}

pub(crate) fn push_filter_query(
    query: &mut sqlx::QueryBuilder<'_, sqlx::Postgres>,
    filters: &[FilterExpr],
) {
    for (index, filter) in filters.iter().enumerate() {
        if index > 0 {
            query.push(" AND ");
        }
        push_filter_expr_query(query, filter);
    }
}

pub(crate) fn push_filter_expr_query(
    query: &mut sqlx::QueryBuilder<'_, sqlx::Postgres>,
    filter: &FilterExpr,
) {
    match filter {
        FilterExpr::Filter(filter) => match filter.op {
            FilterOp::Eq => push_binary_filter_query(query, filter.column, "=", &filter.value),
            FilterOp::Ne => push_binary_filter_query(query, filter.column, "!=", &filter.value),
            FilterOp::Lt => push_binary_filter_query(query, filter.column, "<", &filter.value),
            FilterOp::Lte => push_binary_filter_query(query, filter.column, "<=", &filter.value),
            FilterOp::Gt => push_binary_filter_query(query, filter.column, ">", &filter.value),
            FilterOp::Gte => push_binary_filter_query(query, filter.column, ">=", &filter.value),
            FilterOp::In => {
                query.push(filter.column).push(" IN (");
                let FilterValue::Many(values) = &filter.value else {
                    unreachable!();
                };
                for (value_index, value) in values.iter().enumerate() {
                    if value_index > 0 {
                        query.push(", ");
                    }
                    push_bind_value(query, value);
                }
                query.push(")");
            }
            FilterOp::Contains | FilterOp::StartsWith => {
                push_binary_filter_query(query, filter.column, "LIKE", &filter.value)
            }
            FilterOp::IsNull => {
                query.push(filter.column).push(" IS NULL");
            }
            FilterOp::IsNotNull => {
                query.push(filter.column).push(" IS NOT NULL");
            }
            FilterOp::EqOrNull => {
                let FilterValue::Single(value) = &filter.value else {
                    unreachable!("FilterOp::EqOrNull requires FilterValue::Single");
                };
                query.push("(").push(filter.column).push(" IS NULL OR ");
                query.push(filter.column).push(" = ");
                push_bind_value(query, value);
                query.push(")");
            }
        },
        FilterExpr::All(filters) => push_grouped_filter_query(query, filters, " AND "),
        FilterExpr::Any(filters) => push_grouped_filter_query(query, filters, " OR "),
        FilterExpr::Not(filter) => {
            query.push("NOT (");
            push_filter_expr_query(query, filter);
            query.push(")");
        }
        FilterExpr::Relation(relation) => push_relation_filter_query(query, relation),
        FilterExpr::Coalesce(coalesce) => push_coalesce_filter_query(query, coalesce),
    }
}

fn push_coalesce_filter_query(
    query: &mut sqlx::QueryBuilder<'_, sqlx::Postgres>,
    filter: &cratestack_sql::CoalesceFilter,
) {
    query.push("COALESCE(");
    for (idx, column) in filter.columns.iter().enumerate() {
        if idx > 0 {
            query.push(", ");
        }
        query.push(*column);
    }
    query.push(")");
    match filter.op {
        FilterOp::Eq => push_coalesce_binary(query, "=", &filter.value),
        FilterOp::Ne => push_coalesce_binary(query, "!=", &filter.value),
        FilterOp::Lt => push_coalesce_binary(query, "<", &filter.value),
        FilterOp::Lte => push_coalesce_binary(query, "<=", &filter.value),
        FilterOp::Gt => push_coalesce_binary(query, ">", &filter.value),
        FilterOp::Gte => push_coalesce_binary(query, ">=", &filter.value),
        FilterOp::IsNull => {
            query.push(" IS NULL");
        }
        FilterOp::IsNotNull => {
            query.push(" IS NOT NULL");
        }
        // The remaining FilterOp variants don't have a sensible
        // coalesce semantics — `IN` and `LIKE` against a coalesced
        // tuple would invite footguns, and `EqOrNull` has no LHS
        // column to also null-check. Fail loud on construction
        // (caller built a CoalesceFilter with an unsupported op) so
        // schema drift can't smuggle in a broken predicate.
        FilterOp::In | FilterOp::Contains | FilterOp::StartsWith | FilterOp::EqOrNull => {
            unreachable!(
                "CoalesceFilter built with unsupported op {:?}; only Eq/Ne/Lt/Lte/Gt/Gte/IsNull/IsNotNull are valid",
                filter.op,
            );
        }
    }
}

fn push_coalesce_binary(
    query: &mut sqlx::QueryBuilder<'_, sqlx::Postgres>,
    operator: &str,
    value: &FilterValue,
) {
    query.push(" ").push(operator).push(" ");
    let FilterValue::Single(value) = value else {
        unreachable!("coalesce comparison requires FilterValue::Single");
    };
    push_bind_value(query, value);
}

fn push_relation_filter_query(
    query: &mut sqlx::QueryBuilder<'_, sqlx::Postgres>,
    relation: &RelationFilter,
) {
    match relation.quantifier {
        RelationQuantifier::ToOne | RelationQuantifier::Some => {
            query.push("EXISTS (SELECT 1 FROM ");
            query.push(relation.related_table);
            query.push(" WHERE ");
            query.push(relation.related_table);
            query.push(".");
            query.push(relation.related_column);
            query.push(" = ");
            query.push(relation.parent_table);
            query.push(".");
            query.push(relation.parent_column);
            query.push(" AND ");
            push_filter_expr_query(query, &relation.filter);
            query.push(")");
        }
        RelationQuantifier::None => {
            query.push("NOT EXISTS (SELECT 1 FROM ");
            query.push(relation.related_table);
            query.push(" WHERE ");
            query.push(relation.related_table);
            query.push(".");
            query.push(relation.related_column);
            query.push(" = ");
            query.push(relation.parent_table);
            query.push(".");
            query.push(relation.parent_column);
            query.push(" AND ");
            push_filter_expr_query(query, &relation.filter);
            query.push(")");
        }
        RelationQuantifier::Every => {
            query.push("NOT EXISTS (SELECT 1 FROM ");
            query.push(relation.related_table);
            query.push(" WHERE ");
            query.push(relation.related_table);
            query.push(".");
            query.push(relation.related_column);
            query.push(" = ");
            query.push(relation.parent_table);
            query.push(".");
            query.push(relation.parent_column);
            query.push(" AND NOT (");
            push_filter_expr_query(query, &relation.filter);
            query.push("))");
        }
    }
}

pub(crate) fn push_action_policy_query(
    query: &mut sqlx::QueryBuilder<'_, sqlx::Postgres>,
    allow_policies: &[ReadPolicy],
    deny_policies: &[ReadPolicy],
    ctx: &CoolContext,
) {
    if !deny_policies.is_empty() {
        query.push("NOT (");
        push_allow_policy_query(query, deny_policies, ctx);
        query.push(") AND (");
        push_allow_policy_query(query, allow_policies, ctx);
        query.push(")");
    } else {
        push_allow_policy_query(query, allow_policies, ctx);
    }
}

fn push_allow_policy_query(
    query: &mut sqlx::QueryBuilder<'_, sqlx::Postgres>,
    policies: &[ReadPolicy],
    ctx: &CoolContext,
) {
    if policies.is_empty() {
        query.push("FALSE");
        return;
    }

    for (policy_index, policy) in policies.iter().enumerate() {
        if policy_index > 0 {
            query.push(" OR ");
        }
        push_policy_expr_query(query, policy.expr, ctx);
    }
}

pub(crate) fn push_policy_expr_query(
    query: &mut sqlx::QueryBuilder<'_, sqlx::Postgres>,
    expr: PolicyExpr,
    ctx: &CoolContext,
) {
    match expr {
        PolicyExpr::Predicate(predicate) => match predicate {
            ReadPredicate::AuthNotNull => {
                query.push(if ctx.is_authenticated() {
                    "TRUE"
                } else {
                    "FALSE"
                });
            }
            ReadPredicate::AuthIsNull => {
                query.push(if ctx.is_authenticated() {
                    "FALSE"
                } else {
                    "TRUE"
                });
            }
            ReadPredicate::HasRole { role } => {
                query.push(if context_has_role(ctx, role) {
                    "TRUE"
                } else {
                    "FALSE"
                });
            }
            ReadPredicate::InTenant { tenant_id } => {
                query.push(if context_in_tenant(ctx, tenant_id) {
                    "TRUE"
                } else {
                    "FALSE"
                });
            }
            ReadPredicate::AuthFieldEqLiteral { auth_field, value } => {
                query.push(
                    if ctx
                        .auth_field(auth_field)
                        .is_some_and(|candidate| value_matches_auth_literal(candidate, value))
                    {
                        "TRUE"
                    } else {
                        "FALSE"
                    },
                );
            }
            ReadPredicate::AuthFieldNeLiteral { auth_field, value } => {
                query.push(
                    if ctx
                        .auth_field(auth_field)
                        .is_some_and(|candidate| !value_matches_auth_literal(candidate, value))
                    {
                        "TRUE"
                    } else {
                        "FALSE"
                    },
                );
            }
            ReadPredicate::FieldIsTrue { column } => {
                query.push(column).push(" = TRUE");
            }
            ReadPredicate::FieldEqLiteral { column, value } => {
                query.push(column).push(" = ");
                push_policy_literal(query, value);
            }
            ReadPredicate::FieldNeLiteral { column, value } => {
                query.push(column).push(" != ");
                push_policy_literal(query, value);
            }
            ReadPredicate::FieldEqAuth { column, auth_field } => {
                if let Some(value) = auth_value_to_sql(ctx, auth_field) {
                    query.push(column).push(" = ");
                    push_bind_value(query, &value);
                } else {
                    query.push("FALSE");
                }
            }
            ReadPredicate::FieldNeAuth { column, auth_field } => {
                if let Some(value) = auth_value_to_sql(ctx, auth_field) {
                    query.push(column).push(" != ");
                    push_bind_value(query, &value);
                } else {
                    query.push("FALSE");
                }
            }
            ReadPredicate::Relation {
                quantifier,
                parent_table,
                parent_column,
                related_table,
                related_column,
                expr,
            } => push_relation_policy_query(
                query,
                quantifier,
                parent_table,
                parent_column,
                related_table,
                related_column,
                expr,
                ctx,
            ),
        },
        PolicyExpr::And(exprs) => push_grouped_policy_query(query, exprs, " AND ", ctx),
        PolicyExpr::Or(exprs) => push_grouped_policy_query(query, exprs, " OR ", ctx),
    }
}

fn push_relation_policy_query(
    query: &mut sqlx::QueryBuilder<'_, sqlx::Postgres>,
    quantifier: RelationQuantifier,
    parent_table: &'static str,
    parent_column: &'static str,
    related_table: &'static str,
    related_column: &'static str,
    expr: &'static PolicyExpr,
    ctx: &CoolContext,
) {
    match quantifier {
        RelationQuantifier::ToOne | RelationQuantifier::Some => {
            push_relation_policy_exists_query(
                query,
                parent_table,
                parent_column,
                related_table,
                related_column,
                &|query| push_policy_expr_query(query, *expr, ctx),
            );
        }
        RelationQuantifier::None => {
            query.push("NOT EXISTS (SELECT 1 FROM ");
            query.push(related_table);
            query.push(" WHERE ");
            query.push(related_table);
            query.push(".");
            query.push(related_column);
            query.push(" = ");
            query.push(parent_table);
            query.push(".");
            query.push(parent_column);
            query.push(" AND ");
            push_policy_expr_query(query, *expr, ctx);
            query.push(")");
        }
        RelationQuantifier::Every => {
            query.push("NOT EXISTS (SELECT 1 FROM ");
            query.push(related_table);
            query.push(" WHERE ");
            query.push(related_table);
            query.push(".");
            query.push(related_column);
            query.push(" = ");
            query.push(parent_table);
            query.push(".");
            query.push(parent_column);
            query.push(" AND NOT (");
            push_policy_expr_query(query, *expr, ctx);
            query.push("))");
        }
    }
}

fn push_relation_policy_exists_query<Render>(
    query: &mut sqlx::QueryBuilder<'_, sqlx::Postgres>,
    parent_table: &'static str,
    parent_column: &'static str,
    related_table: &'static str,
    related_column: &'static str,
    render_predicate: &Render,
) where
    Render: Fn(&mut sqlx::QueryBuilder<'_, sqlx::Postgres>),
{
    query.push("EXISTS (SELECT 1 FROM ");
    query.push(related_table);
    query.push(" WHERE ");
    query.push(related_table);
    query.push(".");
    query.push(related_column);
    query.push(" = ");
    query.push(parent_table);
    query.push(".");
    query.push(parent_column);
    query.push(" AND ");
    render_predicate(query);
    query.push(")");
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
            push_order_clause_query(query, clause);
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

fn push_order_clause_query(
    query: &mut sqlx::QueryBuilder<'_, sqlx::Postgres>,
    clause: &OrderClause,
) {
    match &clause.target {
        OrderTarget::Column(column) => {
            query
                .push(*column)
                .push(" ")
                .push(sort_direction_sql(clause.direction))
                .push(" ")
                .push(null_order_sql(clause.null_order));
        }
        OrderTarget::RelationScalar {
            parent_table,
            parent_column,
            related_table,
            related_column,
            value_sql,
        } => {
            query
                .push("(SELECT ")
                .push(*value_sql)
                .push(" FROM ")
                .push(*related_table)
                .push(" WHERE ")
                .push(*related_table)
                .push(".")
                .push(*related_column)
                .push(" = ")
                .push(*parent_table)
                .push(".")
                .push(*parent_column)
                .push(" LIMIT 1) ")
                .push(sort_direction_sql(clause.direction))
                .push(" ")
                .push(null_order_sql(clause.null_order));
        }
    }
}

fn push_policy_literal(query: &mut sqlx::QueryBuilder<'_, sqlx::Postgres>, literal: PolicyLiteral) {
    match literal {
        PolicyLiteral::Bool(value) => query.push_bind(value),
        PolicyLiteral::Int(value) => query.push_bind(value),
        PolicyLiteral::String(value) => query.push_bind(value.to_owned()),
    };
}

pub(crate) fn auth_value_to_sql(ctx: &CoolContext, auth_field: &str) -> Option<SqlValue> {
    match ctx.auth_field(auth_field)? {
        Value::Bool(value) => Some(SqlValue::Bool(*value)),
        Value::Int(value) => Some(SqlValue::Int(*value)),
        Value::String(value) => Some(SqlValue::String(value.clone())),
        _ => None,
    }
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
        .push(descriptor.table_name)
        .push(" WHERE ")
        .push(descriptor.primary_key)
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

pub(crate) fn value_matches_auth_literal(value: &Value, literal: PolicyLiteral) -> bool {
    match (value, literal) {
        (Value::Bool(left), PolicyLiteral::Bool(right)) => *left == right,
        (Value::Int(left), PolicyLiteral::Int(right)) => *left == right,
        (Value::String(left), PolicyLiteral::String(right)) => left == right,
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

pub(crate) fn sort_direction_sql(direction: SortDirection) -> &'static str {
    match direction {
        SortDirection::Asc => "ASC",
        SortDirection::Desc => "DESC",
    }
}

pub(crate) fn null_order_sql(order: cratestack_sql::NullOrder) -> &'static str {
    match order {
        cratestack_sql::NullOrder::First => "NULLS FIRST",
        cratestack_sql::NullOrder::Last => "NULLS LAST",
    }
}

fn push_binary_filter_query(
    query: &mut sqlx::QueryBuilder<'_, sqlx::Postgres>,
    column: &str,
    operator: &str,
    value: &FilterValue,
) {
    query.push(column).push(" ").push(operator).push(" ");
    let FilterValue::Single(value) = value else {
        unreachable!();
    };
    push_bind_value(query, value);
}

fn push_grouped_filter_query(
    query: &mut sqlx::QueryBuilder<'_, sqlx::Postgres>,
    filters: &[FilterExpr],
    joiner: &str,
) {
    query.push("(");
    for (index, filter) in filters.iter().enumerate() {
        if index > 0 {
            query.push(joiner);
        }
        push_filter_expr_query(query, filter);
    }
    query.push(")");
}

fn push_grouped_policy_query(
    query: &mut sqlx::QueryBuilder<'_, sqlx::Postgres>,
    exprs: &[PolicyExpr],
    joiner: &str,
    ctx: &CoolContext,
) {
    query.push("(");
    for (index, expr) in exprs.iter().enumerate() {
        if index > 0 {
            query.push(joiner);
        }
        push_policy_expr_query(query, *expr, ctx);
    }
    query.push(")");
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
            .is_some_and(|candidate| value_matches_auth_literal(candidate, value)),
        ReadPredicate::AuthFieldNeLiteral { auth_field, value } => ctx
            .auth_field(auth_field)
            .is_some_and(|candidate| !value_matches_auth_literal(candidate, value)),
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
                match quantifier {
                    RelationQuantifier::ToOne | RelationQuantifier::Some => {
                        query.push("EXISTS (SELECT 1 FROM ");
                        query.push(related_table);
                        query.push(" WHERE ");
                        query.push(related_table);
                        query.push(".");
                        query.push(related_column);
                        query.push(" = ");
                        push_bind_value(&mut query, parent_value);
                        query.push(" AND ");
                        push_policy_expr_query(&mut query, *expr, ctx);
                        query.push(")");
                    }
                    RelationQuantifier::None => {
                        query.push("NOT EXISTS (SELECT 1 FROM ");
                        query.push(related_table);
                        query.push(" WHERE ");
                        query.push(related_table);
                        query.push(".");
                        query.push(related_column);
                        query.push(" = ");
                        push_bind_value(&mut query, parent_value);
                        query.push(" AND ");
                        push_policy_expr_query(&mut query, *expr, ctx);
                        query.push(")");
                    }
                    RelationQuantifier::Every => {
                        query.push("NOT EXISTS (SELECT 1 FROM ");
                        query.push(related_table);
                        query.push(" WHERE ");
                        query.push(related_table);
                        query.push(".");
                        query.push(related_column);
                        query.push(" = ");
                        push_bind_value(&mut query, parent_value);
                        query.push(" AND NOT (");
                        push_policy_expr_query(&mut query, *expr, ctx);
                        query.push("))");
                    }
                }

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
