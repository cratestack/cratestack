//! Self-relations (`users.manager_id -> users.id`): the relation subquery
//! must correlate with the outer row through a derived table, for filters
//! and sorts alike, with the related read scope still applied.

use crate::tests_relation_scope::USERS_SCOPE_SQL;
use crate::tests_relation_scope_fixtures::{
    USERS, caller, executed_filter, executed_order, name_eq,
};
use crate::{FilterExpr, OrderClause, SortDirection};

/// `FROM users WHERE users.id = users.manager_id` would compare the inner
/// row with itself; the parent key is captured by a derived table instead.
#[test]
fn a_self_relation_correlates_through_a_derived_table() {
    let self_join = "FROM (SELECT users.manager_id AS cratestack_parent_key) AS \
                     cratestack_self_parent, users WHERE users.id = \
                     cratestack_self_parent.cratestack_parent_key";
    let filter = FilterExpr::relation("users", "manager_id", "users", "id", name_eq("a"), USERS);
    assert_eq!(
        executed_filter(&filter, &caller()),
        format!("EXISTS (SELECT 1 {self_join} AND {USERS_SCOPE_SQL} AND name = $2)"),
    );
    let clause = OrderClause::relation_scalar(
        "users",
        "manager_id",
        "users",
        "id",
        "name",
        USERS,
        SortDirection::Asc,
    );
    assert_eq!(
        executed_order(&clause, &caller()),
        format!("(SELECT users.name {self_join} AND {USERS_SCOPE_SQL} LIMIT 1) ASC NULLS LAST"),
    );
}
