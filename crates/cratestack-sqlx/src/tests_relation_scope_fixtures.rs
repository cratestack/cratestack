//! Shared fixtures for the relation-scope SQL tests
//! (`tests_relation_scope`, `tests_relation_scope_parity`): related read
//! scopes with every shape the splice has to handle, and helpers that run
//! the *executed* pushers and the *preview* renderers over the same input.

use cratestack_core::{CratestackContext, Value};

use crate::query::{push_filter_query, push_order_and_paging};
use crate::render::{render_filter_expr_sql, render_order_clause_sql};
use crate::{
    FieldRef, FilterExpr, OrderClause, PolicyExpr, ReadPolicy, ReadPredicate, RelatedReadScope,
    RelationHop, RelationQuantifier, sqlx,
};

static OWNER_ALLOW: [ReadPolicy; 1] = [ReadPolicy {
    expr: PolicyExpr::Predicate(ReadPredicate::FieldEqAuth {
        column: "owner_id",
        auth_field: "id",
    }),
}];
static BLOCKED_DENY: [ReadPolicy; 1] = [ReadPolicy {
    expr: PolicyExpr::Predicate(ReadPredicate::FieldEqAuth {
        column: "blocked_by",
        auth_field: "id",
    }),
}];
static SIGNED_IN_ALLOW: [ReadPolicy; 1] = [ReadPolicy {
    expr: PolicyExpr::Predicate(ReadPredicate::AuthNotNull),
}];

/// `@@soft_delete` + `@@allow("read", ownerId == auth().id)`.
pub(crate) const USERS: RelatedReadScope = RelatedReadScope::Policy {
    allow: &OWNER_ALLOW,
    deny: &[],
    soft_delete_column: Some("deleted_at"),
};
/// `@@allow("read", auth() != null)` + `@@deny("read", blockedBy == auth().id)`.
pub(crate) const COMMENTS: RelatedReadScope = RelatedReadScope::Policy {
    allow: &SIGNED_IN_ALLOW,
    deny: &BLOCKED_DENY,
    soft_delete_column: None,
};
/// Allow *and* deny both bind the caller's id: the deny's slot comes first.
pub(crate) const LEDGERS: RelatedReadScope = RelatedReadScope::Policy {
    allow: &OWNER_ALLOW,
    deny: &BLOCKED_DENY,
    soft_delete_column: None,
};
/// A deny rule and no allow rule: still default deny, deny slot still bound.
pub(crate) const TOMBS: RelatedReadScope = RelatedReadScope::Policy {
    allow: &[],
    deny: &BLOCKED_DENY,
    soft_delete_column: Some("deleted_at"),
};
/// No read rule at all: default deny.
pub(crate) const VAULTS: RelatedReadScope = RelatedReadScope::Policy {
    allow: &[],
    deny: &[],
    soft_delete_column: None,
};

pub(crate) fn caller() -> CratestackContext {
    CratestackContext::authenticated([("id".to_owned(), Value::Int(7))])
}

pub(crate) fn hop(
    parent: (&'static str, &'static str),
    related: (&'static str, &'static str),
    quantifier: RelationQuantifier,
    scope: RelatedReadScope,
) -> RelationHop {
    RelationHop::new(parent.0, parent.1, related.0, related.1, quantifier, scope)
}

pub(crate) fn name_eq(value: &str) -> FilterExpr {
    FieldRef::<(), String>::new("name")
        .eq(value.to_owned())
        .into()
}

/// The WHERE text `push_filter_query` sends to Postgres.
pub(crate) fn executed_filter(filter: &FilterExpr, ctx: &CratestackContext) -> String {
    let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("");
    push_filter_query(&mut query, std::slice::from_ref(filter), ctx);
    query.sql().as_str().to_owned()
}

/// The WHERE text `preview_scoped_sql` renders for the same filter.
pub(crate) fn previewed_filter(filter: &FilterExpr, ctx: &CratestackContext) -> String {
    let (mut sql, mut bind_index) = (String::new(), 1usize);
    render_filter_expr_sql(filter, &mut sql, &mut bind_index, Some(ctx));
    sql
}

/// The ORDER BY text `push_order_and_paging` sends (no paging).
pub(crate) fn executed_order(clause: &OrderClause, ctx: &CratestackContext) -> String {
    let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("");
    push_order_and_paging(&mut query, std::slice::from_ref(clause), None, None, ctx);
    query
        .sql()
        .as_str()
        .strip_prefix(" ORDER BY ")
        .expect("order clause")
        .to_owned()
}

pub(crate) fn previewed_order(clause: &OrderClause, ctx: &CratestackContext) -> String {
    let (mut sql, mut bind_index) = (String::new(), 1usize);
    render_order_clause_sql(clause, &mut sql, &mut bind_index, Some(ctx));
    sql
}
