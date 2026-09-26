//! String rendering of relation subqueries — relation filters
//! (`EXISTS`/`NOT EXISTS`) and relation sort values (`(SELECT ... LIMIT 1)`)
//! — for the `preview_*` builders. Mirrors, clause for clause and bind for
//! bind, what `query::support::{filter, relation_scope}` push into the
//! executed `QueryBuilder`; `crate::tests_relation_scope_parity` holds the
//! two to byte equality.
//!
//! `ctx` is `None` for the ctx-free `preview_sql`, which renders no policy
//! or soft-delete scope at all — not the root model's, and so not a related
//! model's either. `preview_scoped_sql` passes `Some(ctx)` and renders
//! each subquery's related read scope exactly as it executes.

use std::fmt::Write;

use cratestack_core::CratestackContext;
use cratestack_sql::{RelatedReadScope, RelationHop};

use crate::{RelationFilter, RelationQuantifier};

use super::filter::render_filter_expr_sql;
use super::policy::render_read_policy_sql;

/// Alias of the one-row derived table a self-relation hop correlates
/// through, and its single column. Deliberately unlikely names: the
/// column is visible unqualified inside the subquery, so it must not
/// collide with a model column.
const SELF_PARENT_ALIAS: &str = "cratestack_self_parent";
const SELF_PARENT_KEY: &str = "cratestack_parent_key";

/// `FROM <related> WHERE <related>.<related_column> = <parent>.<parent_column>`.
///
/// For a self-relation (`related == parent`, e.g. `User.manager`) the
/// subquery's own `FROM users` shadows the outer `users`, so that form
/// would compare the inner row with itself. The parent's key is instead
/// captured by a one-row derived table listed *before* the related table:
/// a non-`LATERAL` derived table cannot see its sibling `FROM` items, so
/// `<parent>.<parent_column>` inside it resolves to the enclosing query,
/// while every later reference to `<related>` still means the inner row.
pub(crate) fn relation_from_sql(
    parent_table: &str,
    parent_column: &str,
    related_table: &str,
    related_column: &str,
) -> String {
    if parent_table == related_table {
        format!(
            "FROM (SELECT {parent_table}.{parent_column} AS {SELF_PARENT_KEY}) AS \
             {SELF_PARENT_ALIAS}, {related_table} WHERE {related_table}.{related_column} = \
             {SELF_PARENT_ALIAS}.{SELF_PARENT_KEY}"
        )
    } else {
        format!(
            "FROM {related_table} WHERE {related_table}.{related_column} = \
             {parent_table}.{parent_column}"
        )
    }
}

pub(crate) fn render_relation_filter_sql(
    relation: &RelationFilter,
    sql: &mut String,
    bind_index: &mut usize,
    ctx: Option<&CratestackContext>,
) {
    let (open, negate) = match relation.quantifier {
        RelationQuantifier::ToOne | RelationQuantifier::Some => ("EXISTS (SELECT 1 ", false),
        RelationQuantifier::None => ("NOT EXISTS (SELECT 1 ", false),
        RelationQuantifier::Every => ("NOT EXISTS (SELECT 1 ", true),
    };
    sql.push_str(open);
    sql.push_str(&relation_from_sql(
        relation.parent_table,
        relation.parent_column,
        relation.related_table,
        relation.related_column,
    ));
    render_related_scope_sql(relation.related_table, relation.scope, ctx, sql, bind_index);
    sql.push_str(if negate { " AND NOT (" } else { " AND " });
    render_filter_expr_sql(&relation.filter, sql, bind_index, ctx);
    sql.push_str(if negate { "))" } else { ")" });
}

/// `(SELECT <value> FROM <hop.related> WHERE <join> <scope> LIMIT 1)`,
/// nested once per hop. `NULL` for an empty path.
pub(crate) fn render_relation_value_sql(
    hops: &[RelationHop],
    column: &str,
    sql: &mut String,
    bind_index: &mut usize,
    ctx: Option<&CratestackContext>,
) {
    let Some((hop, rest)) = hops.split_first() else {
        sql.push_str("NULL");
        return;
    };
    sql.push_str("(SELECT ");
    if rest.is_empty() {
        let _ = write!(sql, "{}.{}", hop.related_table, column);
    } else {
        render_relation_value_sql(rest, column, sql, bind_index, ctx);
    }
    sql.push(' ');
    sql.push_str(&relation_from_sql(
        hop.parent_table,
        hop.parent_column,
        hop.related_table,
        hop.related_column,
    ));
    render_related_scope_sql(hop.related_table, hop.scope, ctx, sql, bind_index);
    sql.push_str(" LIMIT 1)");
}

/// ` AND <related>.<deleted_at> IS NULL AND (<related read policy>)`.
fn render_related_scope_sql(
    related_table: &str,
    scope: RelatedReadScope,
    ctx: Option<&CratestackContext>,
    sql: &mut String,
    bind_index: &mut usize,
) {
    let (
        Some(ctx),
        RelatedReadScope::Policy {
            allow,
            deny,
            soft_delete_column,
        },
    ) = (ctx, scope)
    else {
        return;
    };
    if let Some(column) = soft_delete_column {
        let _ = write!(sql, " AND {related_table}.{column} IS NULL");
    }
    // `render_read_policy_sql` only returns `None` for an allow list it
    // could not render; that must still read as deny, never as no scope.
    let policy = render_read_policy_sql(allow, deny, ctx, bind_index)
        .unwrap_or_else(|| "(FALSE)".to_owned());
    sql.push_str(" AND ");
    sql.push_str(&policy);
}
