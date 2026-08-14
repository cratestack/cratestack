#![cfg(test)]

//! No-database coverage of the per-row-column and relation-quantifier
//! `ReadPredicate` variants — the field-level read-denial half of the
//! coverage split from `tests_read_policy_predicates.rs` (which covers
//! the caller-context-only variants and the default-deny /
//! deny-beats-allow precedence rules). See that file's doc comment for
//! why the evaluator being tested here is `render_read_policy_sql`,
//! not anything in `cratestack-policy` itself.

use crate::{PolicyExpr, PolicyLiteral, ReadPolicy, ReadPredicate, render::render_read_policy_sql};
use cratestack_core::{CratestackContext, Value};

fn render(allow: &[ReadPolicy], deny: &[ReadPolicy], ctx: &CratestackContext) -> Option<String> {
    let mut bind_index = 1usize;
    render_read_policy_sql(allow, deny, ctx, &mut bind_index)
}

/// Field-level read denial: `FieldIsTrue`/`FieldNeLiteral` compile to
/// per-row column comparisons rather than a caller-only `TRUE`/`FALSE`
/// constant — this is what makes a row's OWN data (not just the
/// caller's identity) gate visibility.
#[test]
fn field_is_true_and_field_ne_literal_render_column_comparisons() {
    let ctx = CratestackContext::anonymous();
    let allow_is_true = [ReadPolicy {
        expr: PolicyExpr::Predicate(ReadPredicate::FieldIsTrue {
            column: "published",
        }),
    }];
    assert_eq!(
        render(&allow_is_true, &[], &ctx).unwrap(),
        "(published = TRUE)"
    );

    let allow_ne = [ReadPolicy {
        expr: PolicyExpr::Predicate(ReadPredicate::FieldNeLiteral {
            column: "status",
            value: PolicyLiteral::String("archived"),
        }),
    }];
    assert_eq!(render(&allow_ne, &[], &ctx).unwrap(), "(status != $1)");
}

/// `FieldEqAuth`/`FieldNeAuth` compare a row's column against a claim
/// pulled from the caller's own context (e.g. row-ownership checks).
#[test]
fn field_ne_auth_renders_bound_column_comparison() {
    let ctx = CratestackContext::authenticated([(
        "email".to_owned(),
        Value::String("owner@example.com".to_owned()),
    )]);
    let allow = [ReadPolicy {
        expr: PolicyExpr::Predicate(ReadPredicate::FieldNeAuth {
            column: "email",
            auth_field: "email",
        }),
    }];
    assert_eq!(render(&allow, &[], &ctx).unwrap(), "(email != $1)");
}

/// Every `RelationQuantifier` variant, rendered for a `ReadPolicy`
/// (not a `FilterExpr` — see `tests_relation.rs` for that side).
/// `ToOne`/`Some` share the `EXISTS` shape; `None` is `NOT EXISTS`;
/// `Every` is `NOT EXISTS (... AND NOT (...))` — the standard
/// relational-algebra encoding of universal quantification as a
/// negated existential. Because SQL's `EXISTS`/`NOT EXISTS` are
/// evaluated by Postgres against the actual row set at query time,
/// the empty-relation edge case each quantifier must get right (no
/// related rows: `some`/`toOne` must deny, `none`/`every` must allow)
/// falls out of `EXISTS`'s own, DB-guaranteed semantics on an empty
/// result set — `EXISTS (<empty>)` is always `FALSE`, so `NOT EXISTS
/// (<empty>)` is always `TRUE` — rather than anything this crate
/// computes itself. `crates/cratestack-pg/tests/policy_db_recursive.rs`
/// exercises this live against Postgres (`every.active` over a project
/// with an inactive member vs. one with none).
#[test]
fn every_relation_quantifier_variant_renders_its_own_sql_shape() {
    let ctx = CratestackContext::anonymous();
    let policy_for = |quantifier: crate::RelationQuantifier| {
        [ReadPolicy {
            expr: PolicyExpr::Predicate(ReadPredicate::Relation {
                quantifier,
                parent_table: "users",
                parent_column: "id",
                related_table: "sessions",
                related_column: "user_id",
                expr: &PolicyExpr::Predicate(ReadPredicate::FieldIsTrue { column: "active" }),
            }),
        }]
    };

    assert_eq!(
        render(&policy_for(crate::RelationQuantifier::ToOne), &[], &ctx).unwrap(),
        "(EXISTS (SELECT 1 FROM sessions WHERE sessions.user_id = users.id AND active = TRUE))"
    );
    assert_eq!(
        render(&policy_for(crate::RelationQuantifier::Some), &[], &ctx).unwrap(),
        "(EXISTS (SELECT 1 FROM sessions WHERE sessions.user_id = users.id AND active = TRUE))"
    );
    assert_eq!(
        render(&policy_for(crate::RelationQuantifier::None), &[], &ctx).unwrap(),
        "(NOT EXISTS (SELECT 1 FROM sessions WHERE sessions.user_id = users.id AND active = TRUE))"
    );
    assert_eq!(
        render(&policy_for(crate::RelationQuantifier::Every), &[], &ctx).unwrap(),
        "(NOT EXISTS (SELECT 1 FROM sessions WHERE sessions.user_id = users.id AND NOT (active = TRUE)))"
    );
}
