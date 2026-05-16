//! Relation-policy pusher (`EXISTS (SELECT 1 FROM ...)`) for
//! `some`/`every`/`none` quantifiers, plus the reusable `EXISTS`
//! emitter.

use cratestack_core::CoolContext;

use crate::{PolicyExpr, RelationQuantifier, sqlx};

use super::policy::push_policy_expr_query;

#[allow(clippy::too_many_arguments)]
pub(super) fn push_relation_policy_query(
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
