#![cfg(test)]

//! `hasRole` over the principal contract's `roles` (ADR 0021 §2): the
//! list, the legacy `role` claim and the legacy `actor.role` each grant on
//! their own, and a mistyped source never grants. The procedure path is
//! checked here; the SQL-lowered read path and the create-time input check
//! are in `cratestack-sqlx`'s `tests_has_role_contract.rs`.

use std::collections::BTreeMap;

use cratestack_core::{CratestackContext, Value};

use crate::{
    ProcedureArgs, ProcedurePolicy, ProcedurePolicyExpr, ProcedurePredicate, authorize_procedure,
    context_has_role,
};

struct NoArgs;
impl ProcedureArgs for NoArgs {
    fn procedure_arg_value(&self, _field: &str) -> Option<Value> {
        None
    }
}

fn allows(role: &'static str, ctx: &CratestackContext) -> bool {
    let allow = [ProcedurePolicy {
        expr: ProcedurePolicyExpr::Predicate(ProcedurePredicate::HasRole { role }),
    }];
    let granted = authorize_procedure(&allow, &[], &NoArgs, ctx).is_ok();
    assert_eq!(granted, context_has_role(ctx, role), "{ctx:?}");
    granted
}

fn string(value: &str) -> Value {
    Value::String(value.to_owned())
}

#[test]
fn roles_grants_each_member() {
    let ctx = CratestackContext::builder("u")
        .roles(["admin", "teller"])
        .build();
    assert!(allows("admin", &ctx));
    assert!(allows("teller", &ctx));
    assert!(!allows("auditor", &ctx));
    let none = CratestackContext::builder("u").roles::<_, &str>([]).build();
    assert!(!allows("admin", &none));
}

#[test]
fn the_legacy_reads_still_grant_alongside_roles() {
    let ctx = CratestackContext::authenticated([
        ("roles".to_owned(), Value::List(vec![string("teller")])),
        ("role".to_owned(), string("user")),
        (
            "actor".to_owned(),
            Value::Map(BTreeMap::from([("role".to_owned(), string("merchant"))])),
        ),
    ]);
    assert!(allows("teller", &ctx));
    assert!(allows("user", &ctx));
    // Before the contract, a present `role` hid `actor.role`; each source
    // now grants on its own.
    assert!(allows("merchant", &ctx));
    assert!(!allows("admin", &ctx));
}

#[test]
fn a_mistyped_roles_never_grants() {
    let as_string = CratestackContext::authenticated([("roles".to_owned(), string("admin"))]);
    assert!(!allows("admin", &as_string));
    let mixed = CratestackContext::authenticated([(
        "roles".to_owned(),
        Value::List(vec![string("admin"), Value::Int(1)]),
    )]);
    assert!(!allows("admin", &mixed));
    let nested = CratestackContext::authenticated([(
        "roles".to_owned(),
        Value::List(vec![Value::List(vec![string("admin")])]),
    )]);
    assert!(!allows("admin", &nested));
    let int_role = CratestackContext::authenticated([("role".to_owned(), Value::Int(1))]);
    assert!(!allows("1", &int_role));
    assert!(!allows("admin", &CratestackContext::anonymous()));
}
