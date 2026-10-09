//! Recursive policy-expression evaluator used by the create path.
//! Most predicates resolve synchronously against the prospective
//! input values + `ctx`; the relation variant fires an `EXISTS` probe
//! to verify the FK target satisfies the related model's policy.
//!
//! The result is a three-valued [`Truth`], combined by Kleene's `and` / `or`
//! (see [`super::comparison`]): `False` decides an `and` and `True` an `or`,
//! so the evaluation still stops there, but `Unknown` does not stop it.

use cratestack_core::{CratestackContext, CratestackError};

use crate::{PolicyExpr, ReadPredicate, RelationQuantifier, SqlColumnValue, SqlValue, sqlx};

use super::comparison::Truth;
use super::create::evaluate_input_truth;
use super::db::PolicyDb;
use super::policy::push_policy_expr_query;
use super::values::{find_column_value, push_bind_value};

pub(super) fn evaluate_create_policy_expr<'a>(
    mut db: PolicyDb<'a>,
    expr: PolicyExpr,
    values: &'a [SqlColumnValue],
    ctx: &'a CratestackContext,
) -> core::pin::Pin<
    Box<dyn core::future::Future<Output = Result<Truth, CratestackError>> + Send + 'a>,
> {
    Box::pin(async move {
        match expr {
            PolicyExpr::Predicate(predicate) => {
                evaluate_create_predicate(db, predicate, values, ctx).await
            }
            PolicyExpr::And(exprs) => {
                let mut result = Truth::True;
                for expr in exprs.iter().copied() {
                    let operand =
                        evaluate_create_policy_expr(db.reborrow(), expr, values, ctx).await?;
                    result = result.and(operand);
                    if result.is_false() {
                        break;
                    }
                }
                Ok(result)
            }
            PolicyExpr::Or(exprs) => {
                let mut result = Truth::False;
                for expr in exprs.iter().copied() {
                    let operand =
                        evaluate_create_policy_expr(db.reborrow(), expr, values, ctx).await?;
                    result = result.or(operand);
                    if result.is_true() {
                        break;
                    }
                }
                Ok(result)
            }
        }
    })
}

fn evaluate_create_predicate<'a>(
    db: PolicyDb<'a>,
    predicate: ReadPredicate,
    values: &'a [SqlColumnValue],
    ctx: &'a CratestackContext,
) -> core::pin::Pin<
    Box<dyn core::future::Future<Output = Result<Truth, CratestackError>> + Send + 'a>,
> {
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
                    return Ok(Truth::False);
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

                let built = query.build_query_as::<(bool,)>();
                let result: (bool,) = match db {
                    PolicyDb::Pool(pool) => built.fetch_one(pool).await,
                    PolicyDb::Conn(conn) => built.fetch_one(conn).await,
                }
                .map_err(crate::error::cratestack_error_from_sqlx)?;
                // `EXISTS` is never NULL, so a relation is decided: its own
                // policy expression is three-valued inside the subquery.
                Ok(result.0.into())
            }
            _ => Ok(evaluate_input_truth(predicate, values, ctx)),
        }
    })
}

fn push_relation_exists(
    query: &mut sqlx::QueryBuilder<sqlx::Postgres>,
    quantifier: RelationQuantifier,
    related_table: &'static str,
    related_column: &'static str,
    parent_value: &SqlValue,
    expr: PolicyExpr,
    ctx: &CratestackContext,
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
