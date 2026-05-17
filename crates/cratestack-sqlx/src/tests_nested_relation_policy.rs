#![cfg(test)]

use crate::{PolicyExpr, ReadPolicy, ReadPredicate, render::render_read_policy_sql};
use cratestack_core::CoolContext;

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
    let ctx = CoolContext::anonymous();

    let mut bind_index = 1usize;
    let sql = render_read_policy_sql(&policy, &[], &ctx, &mut bind_index)
        .expect("policy preview should render");

    assert_eq!(
        sql,
        "EXISTS (SELECT 1 FROM users WHERE users.id = posts.author_id AND NOT EXISTS (SELECT 1 FROM memberships WHERE memberships.user_id = users.id AND NOT (active = $1)))"
    );
}
