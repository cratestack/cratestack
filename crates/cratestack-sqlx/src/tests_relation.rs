#![cfg(test)]

use crate::{
    FieldRef, FilterExpr, OrderClause, PolicyExpr, ReadPolicy, ReadPredicate, SortDirection,
    render::{render_filter_expr_sql, render_order_clause_sql, render_read_policy_sql},
};
use cratestack_core::{CoolContext, Value};

#[test]
fn relation_filters_render_explicit_quantifiers() {
    let some = FilterExpr::relation_some(
        "users",
        "id",
        "sessions",
        "user_id",
        FieldRef::<(), String>::new("label")
            .contains("Revoked")
            .into(),
    );
    let every = FilterExpr::relation_every(
        "users",
        "id",
        "sessions",
        "user_id",
        FieldRef::<(), String>::new("label")
            .contains("Session")
            .into(),
    );
    let none = FilterExpr::relation_none(
        "users",
        "id",
        "sessions",
        "user_id",
        FieldRef::<(), Option<String>>::new("revoked_at")
            .is_not_null()
            .into(),
    );
    let mut bind_index = 1usize;
    let mut some_sql = String::new();
    render_filter_expr_sql(&some, &mut some_sql, &mut bind_index);
    let mut every_sql = String::new();
    render_filter_expr_sql(&every, &mut every_sql, &mut bind_index);
    let mut none_sql = String::new();
    render_filter_expr_sql(&none, &mut none_sql, &mut bind_index);

    assert_eq!(
        some_sql,
        "EXISTS (SELECT 1 FROM sessions WHERE sessions.user_id = users.id AND label LIKE $1)"
    );
    assert_eq!(
        every_sql,
        "NOT EXISTS (SELECT 1 FROM sessions WHERE sessions.user_id = users.id AND NOT (label LIKE $2))"
    );
    assert_eq!(
        none_sql,
        "NOT EXISTS (SELECT 1 FROM sessions WHERE sessions.user_id = users.id AND revoked_at IS NOT NULL)"
    );
}

#[test]
fn relation_scalar_order_preview_uses_correlated_subquery() {
    let clause = OrderClause::relation_scalar(
        "posts",
        "author_id",
        "users",
        "id",
        "users.email",
        SortDirection::Asc,
    );
    let mut sql = String::new();
    render_order_clause_sql(&clause, &mut sql);

    assert_eq!(
        sql,
        "(SELECT users.email FROM users WHERE users.id = posts.author_id LIMIT 1) ASC NULLS LAST"
    );
}

#[test]
fn relation_policy_preview_uses_exists_subquery() {
    let policy = [ReadPolicy {
        expr: PolicyExpr::Predicate(ReadPredicate::Relation {
            quantifier: crate::RelationQuantifier::ToOne,
            parent_table: "posts",
            parent_column: "author_id",
            related_table: "users",
            related_column: "id",
            expr: &PolicyExpr::Predicate(ReadPredicate::FieldEqAuth {
                column: "email",
                auth_field: "email",
            }),
        }),
    }];
    let ctx = CoolContext::authenticated([(
        "email".to_owned(),
        Value::String("owner@example.com".to_owned()),
    )]);

    let mut bind_index = 1usize;
    let sql = render_read_policy_sql(&policy, &[], &ctx, &mut bind_index)
        .expect("policy preview should render");

    assert_eq!(
        sql,
        "EXISTS (SELECT 1 FROM users WHERE users.id = posts.author_id AND email = $1)"
    );
}
