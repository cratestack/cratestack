#![cfg(test)]

//! Coverage for the `InputField*` `ProcedurePredicate` variants — the
//! ones that read from procedure arguments (`args.<field>`) rather than
//! the caller's auth context. Needs a real `ProcedureArgs` impl, unlike
//! `tests_authorize_procedure.rs`'s `NoArgs` stub.

use crate::{
    ProcedureArgs, ProcedurePolicy, ProcedurePolicyExpr, ProcedurePolicyLiteral,
    ProcedurePredicate, authorize_procedure,
};
use cratestack_core::{CratestackContext, Value};
use std::collections::BTreeMap;

struct MapArgs(BTreeMap<&'static str, Value>);

impl ProcedureArgs for MapArgs {
    fn procedure_arg_value(&self, field: &str) -> Option<Value> {
        self.0.get(field).cloned()
    }
}

fn policy(predicate: ProcedurePredicate) -> ProcedurePolicy {
    ProcedurePolicy {
        expr: ProcedurePolicyExpr::Predicate(predicate),
    }
}

#[test]
fn input_field_is_true_variant() {
    let ctx = CratestackContext::authenticated([]);
    let allow = [policy(ProcedurePredicate::InputFieldIsTrue {
        field: "publish",
    })];

    let publishing = MapArgs(BTreeMap::from([("publish", Value::Bool(true))]));
    assert!(authorize_procedure(&allow, &[], &publishing, &ctx).is_ok());

    let not_publishing = MapArgs(BTreeMap::from([("publish", Value::Bool(false))]));
    assert!(authorize_procedure(&allow, &[], &not_publishing, &ctx).is_err());

    // Field absent entirely must not vacuously match, same as `false`.
    let missing = MapArgs(BTreeMap::new());
    assert!(authorize_procedure(&allow, &[], &missing, &ctx).is_err());
}

#[test]
fn input_field_eq_and_ne_literal_variants() {
    let ctx = CratestackContext::authenticated([]);
    let eq_two = [policy(ProcedurePredicate::InputFieldEqLiteral {
        field: "postId",
        value: ProcedurePolicyLiteral::Int(2),
    })];
    let ne_two = [policy(ProcedurePredicate::InputFieldNeLiteral {
        field: "postId",
        value: ProcedurePolicyLiteral::Int(2),
    })];

    let matching = MapArgs(BTreeMap::from([("postId", Value::Int(2))]));
    let other = MapArgs(BTreeMap::from([("postId", Value::Int(3))]));

    assert!(authorize_procedure(&eq_two, &[], &matching, &ctx).is_ok());
    assert!(authorize_procedure(&eq_two, &[], &other, &ctx).is_err());
    assert!(authorize_procedure(&ne_two, &[], &matching, &ctx).is_err());
    assert!(authorize_procedure(&ne_two, &[], &other, &ctx).is_ok());
}

#[test]
fn input_field_eq_and_ne_auth_variants() {
    let owner_ctx = CratestackContext::authenticated([(
        "email".to_owned(),
        Value::String("owner@example.com".to_owned()),
    )]);
    let eq_auth = [policy(ProcedurePredicate::InputFieldEqAuth {
        field: "ownerEmail",
        auth_field: "email",
    })];
    let ne_auth = [policy(ProcedurePredicate::InputFieldNeAuth {
        field: "ownerEmail",
        auth_field: "email",
    })];

    let matching = MapArgs(BTreeMap::from([(
        "ownerEmail",
        Value::String("owner@example.com".to_owned()),
    )]));
    let mismatched = MapArgs(BTreeMap::from([(
        "ownerEmail",
        Value::String("someone-else@example.com".to_owned()),
    )]));

    assert!(authorize_procedure(&eq_auth, &[], &matching, &owner_ctx).is_ok());
    assert!(authorize_procedure(&eq_auth, &[], &mismatched, &owner_ctx).is_err());
    assert!(authorize_procedure(&ne_auth, &[], &matching, &owner_ctx).is_err());
    assert!(authorize_procedure(&ne_auth, &[], &mismatched, &owner_ctx).is_ok());

    // Missing the auth field entirely must not vacuously match.
    let anonymous = CratestackContext::anonymous();
    assert!(authorize_procedure(&eq_auth, &[], &matching, &anonymous).is_err());
}

#[test]
fn input_field_eq_and_ne_input_variants() {
    let ctx = CratestackContext::authenticated([]);
    let eq_input = [policy(ProcedurePredicate::InputFieldEqInput {
        field: "ownerEmail",
        other_field: "mirrorEmail",
    })];
    let ne_input = [policy(ProcedurePredicate::InputFieldNeInput {
        field: "ownerEmail",
        other_field: "mirrorEmail",
    })];

    let matching = MapArgs(BTreeMap::from([
        ("ownerEmail", Value::String("a@example.com".to_owned())),
        ("mirrorEmail", Value::String("a@example.com".to_owned())),
    ]));
    let mismatched = MapArgs(BTreeMap::from([
        ("ownerEmail", Value::String("a@example.com".to_owned())),
        ("mirrorEmail", Value::String("b@example.com".to_owned())),
    ]));

    assert!(authorize_procedure(&eq_input, &[], &matching, &ctx).is_ok());
    assert!(authorize_procedure(&eq_input, &[], &mismatched, &ctx).is_err());
    assert!(authorize_procedure(&ne_input, &[], &matching, &ctx).is_err());
    assert!(authorize_procedure(&ne_input, &[], &mismatched, &ctx).is_ok());
}
