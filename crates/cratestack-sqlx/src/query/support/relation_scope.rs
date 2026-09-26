//! Relation subqueries as they execute: relation filters (`EXISTS` /
//! `NOT EXISTS`) and relation sort values (`(SELECT ... LIMIT 1)`), each
//! with the related model's read scope spliced in (GHSA-p55v-6xv5-93p3).
//!
//! Without the scope, a caller could test (`EXISTS`) or order by values
//! of related rows the related model's policy hides from them — rows
//! `?include=` correctly reports as `null`. With it, a hidden related row
//! behaves as nonexistent, exactly as it does for `include`.
//!
//! Column resolution: the scope's policy columns are unqualified, and SQL
//! resolves an unqualified name to the innermost `FROM` that has it — the
//! related table here, whose own model the policy was written for. The
//! soft-delete column is qualified with the related table explicitly.
//! Self-relations correlate through a derived table; see
//! [`crate::render::relation_from_sql`].
//!
//! `crate::render::relation` is the string twin of this module used by
//! `preview_scoped_sql`; `crate::tests_relation_scope_parity` holds the
//! two to byte equality.

use cratestack_core::CratestackContext;
use cratestack_sql::{RelatedReadScope, RelationHop};

use crate::render::relation_from_sql;
use crate::{RelationFilter, RelationQuantifier, sqlx};

use super::filter::push_filter_expr_query;
use super::policy::push_action_policy_query;

pub(super) fn push_relation_filter_query(
    query: &mut sqlx::QueryBuilder<sqlx::Postgres>,
    relation: &RelationFilter,
    ctx: &CratestackContext,
) {
    let (open, negate) = match relation.quantifier {
        RelationQuantifier::ToOne | RelationQuantifier::Some => ("EXISTS (SELECT 1 ", false),
        RelationQuantifier::None => ("NOT EXISTS (SELECT 1 ", false),
        RelationQuantifier::Every => ("NOT EXISTS (SELECT 1 ", true),
    };
    query.push(open);
    query.push(relation_from_sql(
        relation.parent_table,
        relation.parent_column,
        relation.related_table,
        relation.related_column,
    ));
    push_related_scope(query, relation.related_table, relation.scope, ctx);
    query.push(if negate { " AND NOT (" } else { " AND " });
    push_filter_expr_query(query, &relation.filter, ctx);
    query.push(if negate { "))" } else { ")" });
}

/// `(SELECT <value> FROM <hop.related> WHERE <join> <scope> LIMIT 1)`,
/// nested once per hop, `<value>` being the next hop's subquery or, at the
/// last hop, `related.column`. Every level applies its own hop's scope, so
/// a hidden row anywhere on the path yields NULL. `NULL` for an empty path.
pub(super) fn push_relation_value(
    query: &mut sqlx::QueryBuilder<sqlx::Postgres>,
    hops: &[RelationHop],
    column: &'static str,
    ctx: &CratestackContext,
) {
    let Some((hop, rest)) = hops.split_first() else {
        query.push("NULL");
        return;
    };
    query.push("(SELECT ");
    if rest.is_empty() {
        query.push(hop.related_table).push(".").push(column);
    } else {
        push_relation_value(query, rest, column, ctx);
    }
    query.push(" ");
    query.push(relation_from_sql(
        hop.parent_table,
        hop.parent_column,
        hop.related_table,
        hop.related_column,
    ));
    push_related_scope(query, hop.related_table, hop.scope, ctx);
    query.push(" LIMIT 1)");
}

/// ` AND <related>.<deleted_at> IS NULL AND (<related read policy>)`, or
/// nothing for [`RelatedReadScope::Unscoped`].
fn push_related_scope(
    query: &mut sqlx::QueryBuilder<sqlx::Postgres>,
    related_table: &'static str,
    scope: RelatedReadScope,
    ctx: &CratestackContext,
) {
    let RelatedReadScope::Policy {
        allow,
        deny,
        soft_delete_column,
    } = scope
    else {
        return;
    };
    if let Some(column) = soft_delete_column {
        query
            .push(" AND ")
            .push(related_table)
            .push(".")
            .push(column)
            .push(" IS NULL");
    }
    query.push(" AND ");
    push_action_policy_query(query, allow, deny, ctx);
}
