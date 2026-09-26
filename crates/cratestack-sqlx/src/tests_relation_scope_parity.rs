//! `preview_scoped_sql` must show the SQL that executes. Relation
//! subqueries are rendered twice — by the `QueryBuilder` pushers
//! (`query::support::relation_scope`) and by the string renderer
//! (`render::relation`) — so this holds the two to byte equality,
//! `$n` numbering included, over every scope shape.

use cratestack_core::CratestackContext;

use crate::tests_relation_scope_fixtures::{
    COMMENTS, LEDGERS, TOMBS, USERS, VAULTS, caller, executed_filter, executed_order, hop, name_eq,
    previewed_filter, previewed_order,
};
use crate::{FilterExpr, OrderClause, RelatedReadScope, RelationQuantifier, SortDirection};

fn filters() -> Vec<FilterExpr> {
    let two_hops = [
        hop(
            ("posts", "author_id"),
            ("users", "id"),
            RelationQuantifier::ToOne,
            USERS,
        ),
        hop(
            ("users", "id"),
            ("comments", "user_id"),
            RelationQuantifier::Every,
            COMMENTS,
        ),
    ];
    vec![
        FilterExpr::relation("posts", "author_id", "users", "id", name_eq("a"), USERS),
        FilterExpr::relation_some("posts", "id", "comments", "post_id", name_eq("a"), COMMENTS),
        FilterExpr::relation_every("posts", "id", "comments", "post_id", name_eq("a"), COMMENTS),
        FilterExpr::relation_none("posts", "id", "comments", "post_id", name_eq("a"), COMMENTS),
        FilterExpr::relation("posts", "vault_id", "vaults", "id", name_eq("a"), VAULTS),
        FilterExpr::relation("posts", "ledger_id", "ledgers", "id", name_eq("a"), LEDGERS),
        FilterExpr::relation_some("posts", "id", "tombs", "post_id", name_eq("a"), TOMBS),
        FilterExpr::relation("users", "manager_id", "users", "id", name_eq("a"), USERS),
        FilterExpr::relation(
            "posts",
            "author_id",
            "users",
            "id",
            name_eq("a"),
            RelatedReadScope::Unscoped,
        ),
        crate::wrap_filter(&two_hops, name_eq("a")),
        FilterExpr::any([
            name_eq("root"),
            crate::wrap_filter(&two_hops, name_eq("b")).not(),
        ]),
    ]
}

fn orders() -> Vec<OrderClause> {
    let two_hops = [
        hop(
            ("posts", "author_id"),
            ("users", "id"),
            RelationQuantifier::ToOne,
            USERS,
        ),
        hop(
            ("users", "profile_id"),
            ("profiles", "id"),
            RelationQuantifier::ToOne,
            COMMENTS,
        ),
    ];
    vec![
        OrderClause::relation_scalar(
            "posts",
            "author_id",
            "users",
            "id",
            "email",
            USERS,
            SortDirection::Asc,
        ),
        OrderClause::relation_path(&two_hops, "nickname", SortDirection::Desc),
        OrderClause::relation_scalar(
            "users",
            "manager_id",
            "users",
            "id",
            "name",
            USERS,
            SortDirection::Asc,
        ),
    ]
}

#[test]
fn previewed_relation_filters_match_executed_ones() {
    for ctx in [caller(), CratestackContext::anonymous()] {
        for filter in filters() {
            assert_eq!(
                previewed_filter(&filter, &ctx),
                executed_filter(&filter, &ctx)
            );
        }
    }
}

#[test]
fn previewed_relation_sorts_match_executed_ones() {
    for ctx in [caller(), CratestackContext::anonymous()] {
        for clause in orders() {
            assert_eq!(
                previewed_order(&clause, &ctx),
                executed_order(&clause, &ctx)
            );
        }
    }
}
