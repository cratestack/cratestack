#![cfg(test)]

use crate::{PolicyExpr, ReadPolicy, ReadPredicate, render::render_read_policy_sql};
use cratestack_core::CratestackContext;

#[test]
fn nested_relation_policy_preview_uses_recursive_exists_and_quantifiers() {
    let policy = [ReadPolicy {
        expr: PolicyExpr::Predicate(ReadPredicate::Relation {
            quantifier: crate::RelationQuantifier::ToOne,
            parent_table: "posts",
            parent_column: "author_id",
            related_table: "users",
            related_column: "id",
            expr: &PolicyExpr::Predicate(ReadPredicate::Relation {
                quantifier: crate::RelationQuantifier::Every,
                parent_table: "users",
                parent_column: "id",
                related_table: "memberships",
                related_column: "user_id",
                expr: &PolicyExpr::Predicate(ReadPredicate::FieldEqLiteral {
                    column: "active",
                    value: crate::PolicyLiteral::Bool(true),
                }),
            }),
        }),
    }];
    let ctx = CratestackContext::anonymous();

    let mut bind_index = 1usize;
    let sql = render_read_policy_sql(&policy, &[], &ctx, &mut bind_index)
        .expect("policy preview should render");

    assert_eq!(
        sql,
        "(EXISTS (SELECT 1 FROM users WHERE users.id = posts.author_id AND NOT EXISTS (SELECT 1 FROM memberships WHERE memberships.user_id = users.id AND NOT (active = $1))))"
    );
}

/// A policy traversing a self-relation (`boss.name == "root"` on a model
/// whose `boss` is itself) must correlate with the row being read, not
/// compare the inner row with itself — both when executed and previewed.
#[test]
fn self_relation_policy_correlates_through_a_derived_table() {
    let policy = [ReadPolicy {
        expr: PolicyExpr::Predicate(ReadPredicate::Relation {
            quantifier: crate::RelationQuantifier::ToOne,
            parent_table: "members",
            parent_column: "boss_id",
            related_table: "members",
            related_column: "id",
            expr: &PolicyExpr::Predicate(ReadPredicate::FieldEqLiteral {
                column: "name",
                value: crate::PolicyLiteral::String("root"),
            }),
        }),
    }];
    let ctx = CratestackContext::anonymous();
    let expected = "(EXISTS (SELECT 1 FROM (SELECT members.boss_id AS cratestack_parent_key) AS \
                    cratestack_self_parent, members WHERE members.id = \
                    cratestack_self_parent.cratestack_parent_key AND name = $1))";

    let mut bind_index = 1usize;
    let previewed = render_read_policy_sql(&policy, &[], &ctx, &mut bind_index);
    assert_eq!(previewed.as_deref(), Some(expected));

    let mut query = crate::sqlx::QueryBuilder::<crate::sqlx::Postgres>::new("");
    crate::query::push_action_policy_query(&mut query, &policy, &[], &ctx);
    assert_eq!(query.sql().as_str(), expected);
}
