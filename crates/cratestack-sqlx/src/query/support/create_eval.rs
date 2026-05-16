//! Recursive policy-expression evaluator used by the create path.
//! Most predicates resolve synchronously against the prospective
//! input values + `ctx`; the relation variant fires an `EXISTS` probe
//! to verify the FK target satisfies the related model's policy.

use cratestack_core::{CoolContext, CoolError};

use crate::{PolicyExpr, ReadPredicate, RelationQuantifier, SqlColumnValue, SqlValue, sqlx};

use super::policy::push_policy_expr_query;
use super::values::{find_column_value, push_bind_value};

pub(super) fn evaluate_create_policy_expr<'a>(
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
                push_relation_exists(
                    &mut query,
                    quantifier,
                    related_table,
                    related_column,
                    parent_value,
                    *expr,
                    ctx,
                );

                let result: (bool,) = query
                    .build_query_as::<(bool,)>()
                    .fetch_one(pool)
                    .await
                    .map_err(|error| CoolError::Database(error.to_string()))?;
                Ok(result.0)
            }
            _ => Ok(super::create::evaluate_input_predicate(predicate, values, ctx)),
        }
    })
}

fn push_relation_exists(
    query: &mut sqlx::QueryBuilder<'_, sqlx::Postgres>,
    quantifier: RelationQuantifier,
    related_table: &'static str,
    related_column: &'static str,
    parent_value: &SqlValue,
    expr: PolicyExpr,
    ctx: &CoolContext,
) {
    let (prefix, suffix) = match quantifier {
        RelationQuantifier::ToOne | RelationQuantifier::Some => ("EXISTS (SELECT 1 FROM ", ")"),
        RelationQuantifier::None => ("NOT EXISTS (SELECT 1 FROM ", ")"),
        RelationQuantifier::Every => ("NOT EXISTS (SELECT 1 FROM ", "))"),
    };
    query.push(prefix);
    query.push(related_table);
    query.push(" WHERE ");
    query.push(related_table);
    query.push(".");
    query.push(related_column);
    query.push(" = ");
    push_bind_value(query, parent_value);
    if matches!(quantifier, RelationQuantifier::Every) {
        query.push(" AND NOT (");
    } else {
        query.push(" AND ");
    }
    push_policy_expr_query(query, expr, ctx);
    query.push(suffix);
}
