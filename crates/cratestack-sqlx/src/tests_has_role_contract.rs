#![cfg(test)]

//! `hasRole` over the principal contract's `roles` (ADR 0021 §2) on every
//! path a model policy takes here: the read policy rendered to SQL
//! (`render_read_policy_sql`), the same policy pushed into a
//! `QueryBuilder` (`push_action_policy_query`, the list/update/delete
//! scoping), and the create path's in-process evaluator. All three must
//! agree with `cratestack_policy::context_has_role`, which the procedure
//! path uses (`cratestack-policy`'s `tests_has_role_contract.rs`).

use std::collections::BTreeMap;

use cratestack_core::{CratestackContext, Value};
use cratestack_policy::context_has_role;

use crate::query::evaluate_input_predicate_for_tests as evaluate;
use crate::query::push_action_policy_query;
use crate::{PolicyExpr, ReadPolicy, ReadPredicate, render::render_read_policy_sql, sqlx};

fn string(value: &str) -> Value {
    Value::String(value.to_owned())
}

/// Each path's verdict for `@@allow(.., hasRole(role))`; panics when the
/// paths disagree with each other or with the policy crate.
fn granted(role: &'static str, ctx: &CratestackContext) -> bool {
    let allow = [ReadPolicy {
        expr: PolicyExpr::Predicate(ReadPredicate::HasRole { role }),
    }];
    let mut bind_index = 1usize;
    let rendered = render_read_policy_sql(&allow, &[], ctx, &mut bind_index).unwrap();

    let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("");
    push_action_policy_query(&mut query, &allow, &[], ctx);
    let pushed = query.into_string();

    let created = evaluate(ReadPredicate::HasRole { role }, &[], ctx);
    let expected = context_has_role(ctx, role);
    let sql = if expected { "(TRUE)" } else { "(FALSE)" };
    assert_eq!(rendered, sql, "rendered, {ctx:?}");
    assert_eq!(pushed, sql, "pushed, {ctx:?}");
    assert_eq!(created, expected, "create path, {ctx:?}");
    expected
}

#[test]
fn every_path_reads_roles_and_both_legacy_claims() {
    let ctx = CratestackContext::authenticated([
        ("roles".to_owned(), Value::List(vec![string("teller")])),
        ("role".to_owned(), string("user")),
        (
            "actor".to_owned(),
            Value::Map(BTreeMap::from([("role".to_owned(), string("merchant"))])),
        ),
    ]);
    assert!(granted("teller", &ctx));
    assert!(granted("user", &ctx));
    assert!(granted("merchant", &ctx));
    assert!(!granted("admin", &ctx));

    let built = CratestackContext::builder("u").roles(["admin"]).build();
    assert!(granted("admin", &built));
}

#[test]
fn a_mistyped_roles_denies_on_every_path() {
    let as_string = CratestackContext::authenticated([("roles".to_owned(), string("admin"))]);
    assert!(!granted("admin", &as_string));
    let mixed = CratestackContext::authenticated([(
        "roles".to_owned(),
        Value::List(vec![string("admin"), Value::Bool(true)]),
    )]);
    assert!(!granted("admin", &mixed));
    assert!(!granted("admin", &CratestackContext::anonymous()));
}
