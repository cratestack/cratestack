//! Relation-policy pusher (`EXISTS (SELECT 1 FROM ...)`) for
//! `some`/`every`/`none` quantifiers.
//!
//! The correlation (`FROM <related> WHERE <related>.<col> = <parent>.<col>`)
//! comes from [`crate::render::relation_from_sql`], which correlates a
//! self-relation (`boss.name == "root"` on a model whose `boss` is itself)
//! through a derived table: the plain form binds both sides to the inner
//! row and evaluates the policy uncorrelated with the row being read.

use cratestack_core::CratestackContext;

use crate::render::relation_from_sql;
use crate::{PolicyExpr, RelationQuantifier, sqlx};

use super::policy::push_policy_expr_query;

#[allow(clippy::too_many_arguments)]
pub(super) fn push_relation_policy_query(
    query: &mut sqlx::QueryBuilder<sqlx::Postgres>,
    quantifier: RelationQuantifier,
    parent_table: &'static str,
    parent_column: &'static str,
    related_table: &'static str,
    related_column: &'static str,
    expr: &'static PolicyExpr,
    ctx: &CratestackContext,
) {
    let (open, negate) = match quantifier {
        RelationQuantifier::ToOne | RelationQuantifier::Some => ("EXISTS (SELECT 1 ", false),
        RelationQuantifier::None => ("NOT EXISTS (SELECT 1 ", false),
        RelationQuantifier::Every => ("NOT EXISTS (SELECT 1 ", true),
    };
    query.push(open);
    query.push(relation_from_sql(
        parent_table,
        parent_column,
        related_table,
        related_column,
    ));
    query.push(if negate { " AND NOT (" } else { " AND " });
    push_policy_expr_query(query, *expr, ctx);
    query.push(if negate { "))" } else { ")" });
}
