//! SQL shape of relation subqueries as they execute (GHSA-p55v-6xv5-93p3):
//! every relation filter and relation sort subquery must carry the
//! related model's soft-delete filter and read policy, built through the
//! public constructors a caller would use. `tests_relation_scope_parity`
//! checks the preview renderer produces the same text.

use cratestack_core::CratestackContext;

use crate::tests_relation_scope_fixtures::{
    COMMENTS, LEDGERS, TOMBS, USERS, VAULTS, caller, executed_filter, executed_order, hop, name_eq,
};
use crate::{FilterExpr, OrderClause, RelatedReadScope, RelationQuantifier, SortDirection};

pub(crate) const USERS_SCOPE_SQL: &str = "users.deleted_at IS NULL AND (owner_id = $1)";

fn author(filter: FilterExpr, scope: RelatedReadScope) -> FilterExpr {
    FilterExpr::relation("posts", "author_id", "users", "id", filter, scope)
}

#[test]
fn to_one_filter_splices_soft_delete_and_policy_before_the_predicate() {
    assert_eq!(
        executed_filter(&author(name_eq("ada"), USERS), &caller()),
        format!(
            "EXISTS (SELECT 1 FROM users WHERE users.id = posts.author_id AND {USERS_SCOPE_SQL} \
             AND name = $2)"
        ),
    );
}

#[test]
fn to_many_quantifiers_scope_the_rows_they_quantify_over() {
    let scope = "comments.post_id = posts.id AND (NOT (blocked_by = $1) AND (TRUE))";
    let some =
        FilterExpr::relation_some("posts", "id", "comments", "post_id", name_eq("a"), COMMENTS);
    let every =
        FilterExpr::relation_every("posts", "id", "comments", "post_id", name_eq("a"), COMMENTS);
    let none =
        FilterExpr::relation_none("posts", "id", "comments", "post_id", name_eq("a"), COMMENTS);
    assert_eq!(
        executed_filter(&some, &caller()),
        format!("EXISTS (SELECT 1 FROM comments WHERE {scope} AND name = $2)"),
    );
    // `every`: no *visible* child fails the predicate — hidden children
    // are not counted, so over hidden-only children it is vacuously true.
    assert_eq!(
        executed_filter(&every, &caller()),
        format!("NOT EXISTS (SELECT 1 FROM comments WHERE {scope} AND NOT (name = $2))"),
    );
    assert_eq!(
        executed_filter(&none, &caller()),
        format!("NOT EXISTS (SELECT 1 FROM comments WHERE {scope} AND name = $2)"),
    );
}

#[test]
fn a_related_model_with_no_read_rule_is_default_deny() {
    let filter = FilterExpr::relation("posts", "vault_id", "vaults", "id", name_eq("x"), VAULTS);
    assert_eq!(
        executed_filter(&filter, &caller()),
        "EXISTS (SELECT 1 FROM vaults WHERE vaults.id = posts.vault_id AND (FALSE) AND name = $1)",
    );
}

/// Deny beats allow inside the subquery exactly as on a direct read; the
/// deny's `auth()` bind is numbered first because it is emitted first.
#[test]
fn allow_and_deny_both_apply_with_auth_binds_in_emission_order() {
    let ledger = FilterExpr::relation("posts", "ledger_id", "ledgers", "id", name_eq("a"), LEDGERS);
    assert_eq!(
        executed_filter(&ledger, &caller()),
        "EXISTS (SELECT 1 FROM ledgers WHERE ledgers.id = posts.ledger_id AND \
         (NOT (blocked_by = $1) AND (owner_id = $2)) AND name = $3)",
    );
    let tomb = FilterExpr::relation_some("posts", "id", "tombs", "post_id", name_eq("a"), TOMBS);
    assert_eq!(
        executed_filter(&tomb, &caller()),
        "EXISTS (SELECT 1 FROM tombs WHERE tombs.post_id = posts.id AND tombs.deleted_at IS NULL \
         AND (NOT (blocked_by = $1) AND (FALSE)) AND name = $2)",
    );
}

#[test]
fn an_auth_bound_policy_without_the_auth_field_renders_false() {
    assert_eq!(
        executed_filter(
            &author(name_eq("ada"), USERS),
            &CratestackContext::anonymous()
        ),
        "EXISTS (SELECT 1 FROM users WHERE users.id = posts.author_id AND \
         users.deleted_at IS NULL AND (FALSE) AND name = $1)",
    );
}

#[test]
fn multi_hop_filters_apply_each_hops_own_scope_at_its_own_level() {
    let hops = [
        hop(
            ("posts", "author_id"),
            ("users", "id"),
            RelationQuantifier::ToOne,
            USERS,
        ),
        hop(
            ("users", "id"),
            ("comments", "user_id"),
            RelationQuantifier::Some,
            COMMENTS,
        ),
    ];
    assert_eq!(
        executed_filter(&crate::wrap_filter(&hops, name_eq("a")), &caller()),
        format!(
            "EXISTS (SELECT 1 FROM users WHERE users.id = posts.author_id AND {USERS_SCOPE_SQL} \
             AND EXISTS (SELECT 1 FROM comments WHERE comments.user_id = users.id AND \
             (NOT (blocked_by = $2) AND (TRUE)) AND name = $3))"
        ),
    );
}

#[test]
fn relation_sort_reads_null_for_a_hidden_row() {
    let clause = OrderClause::relation_scalar(
        "posts",
        "author_id",
        "users",
        "id",
        "email",
        USERS,
        SortDirection::Asc,
    );
    assert_eq!(
        executed_order(&clause, &caller()),
        format!(
            "(SELECT users.email FROM users WHERE users.id = posts.author_id AND \
             {USERS_SCOPE_SQL} LIMIT 1) ASC NULLS LAST"
        ),
    );
}

#[test]
fn multi_hop_sort_scopes_every_level_and_numbers_inner_binds_first() {
    let hops = [
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
    let clause = OrderClause::relation_path(&hops, "nickname", SortDirection::Desc);
    assert_eq!(
        executed_order(&clause, &caller()),
        "(SELECT (SELECT profiles.nickname FROM profiles WHERE profiles.id = users.profile_id \
         AND (NOT (blocked_by = $1) AND (TRUE)) LIMIT 1) FROM users WHERE users.id = \
         posts.author_id AND users.deleted_at IS NULL AND (owner_id = $2) LIMIT 1) DESC NULLS LAST",
    );
}

#[test]
fn the_unscoped_escape_hatch_reads_the_raw_table() {
    assert_eq!(
        executed_filter(
            &author(name_eq("ada"), RelatedReadScope::Unscoped),
            &caller()
        ),
        "EXISTS (SELECT 1 FROM users WHERE users.id = posts.author_id AND name = $1)",
    );
}
