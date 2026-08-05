#![cfg(test)]

//! Coverage for `HasRole`/`InTenant`/`AuthField*` `ProcedurePredicate`
//! variants, plus the `context_has_role`/`context_in_tenant` helpers
//! they (and the read-policy side, via `cratestack-sqlx`) share. See
//! `tests_authorize_procedure.rs` for the core precedence rules and
//! `Literal`/`AuthNotNull`/`AuthIsNull`.

use crate::{
    ProcedureArgs, ProcedurePolicy, ProcedurePolicyExpr, ProcedurePolicyLiteral,
    ProcedurePredicate, authorize_procedure, context_has_role, context_in_tenant,
};
use cratestack_core::{CoolContext, Value};
use std::collections::BTreeMap;

struct NoArgs;
impl ProcedureArgs for NoArgs {
    fn procedure_arg_value(&self, _field: &str) -> Option<Value> {
        None
    }
}

fn policy(predicate: ProcedurePredicate) -> ProcedurePolicy {
    ProcedurePolicy {
        expr: ProcedurePolicyExpr::Predicate(predicate),
    }
}

#[test]
fn auth_field_eq_and_ne_literal_variants() {
    let banned =
        CoolContext::authenticated([("status".to_owned(), Value::String("banned".to_owned()))]);
    let active =
        CoolContext::authenticated([("status".to_owned(), Value::String("active".to_owned()))]);
    let eq_banned = [policy(ProcedurePredicate::AuthFieldEqLiteral {
        auth_field: "status",
        value: ProcedurePolicyLiteral::String("banned"),
    })];
    let ne_banned = [policy(ProcedurePredicate::AuthFieldNeLiteral {
        auth_field: "status",
        value: ProcedurePolicyLiteral::String("banned"),
    })];

    assert!(authorize_procedure(&eq_banned, &[], &NoArgs, &banned).is_ok());
    assert!(authorize_procedure(&eq_banned, &[], &NoArgs, &active).is_err());
    assert!(authorize_procedure(&ne_banned, &[], &NoArgs, &banned).is_err());
    assert!(authorize_procedure(&ne_banned, &[], &NoArgs, &active).is_ok());
}

#[test]
fn has_role_predicate_matches_authorize_procedure() {
    let admin =
        CoolContext::authenticated([("role".to_owned(), Value::String("admin".to_owned()))]);
    let member =
        CoolContext::authenticated([("role".to_owned(), Value::String("member".to_owned()))]);
    let allow = [policy(ProcedurePredicate::HasRole { role: "admin" })];

    assert!(authorize_procedure(&allow, &[], &NoArgs, &admin).is_ok());
    assert!(authorize_procedure(&allow, &[], &NoArgs, &member).is_err());
}

#[test]
fn in_tenant_predicate_matches_authorize_procedure() {
    let tenant_a = CoolContext::authenticated([(
        "tenant".to_owned(),
        Value::Map(BTreeMap::from([(
            "id".to_owned(),
            Value::String("tenant_a".to_owned()),
        )])),
    )]);
    let tenant_b = CoolContext::authenticated([(
        "tenant".to_owned(),
        Value::Map(BTreeMap::from([(
            "id".to_owned(),
            Value::String("tenant_b".to_owned()),
        )])),
    )]);
    let allow = [policy(ProcedurePredicate::InTenant {
        tenant_id: "tenant_a",
    })];

    assert!(authorize_procedure(&allow, &[], &NoArgs, &tenant_a).is_ok());
    assert!(authorize_procedure(&allow, &[], &NoArgs, &tenant_b).is_err());
}

#[test]
fn has_role_checks_top_level_and_actor_role() {
    let top_level =
        CoolContext::authenticated([("role".to_owned(), Value::String("admin".to_owned()))]);
    assert!(context_has_role(&top_level, "admin"));
    assert!(!context_has_role(&top_level, "member"));

    let actor_role = CoolContext::authenticated([(
        "actor".to_owned(),
        Value::Map(BTreeMap::from([(
            "role".to_owned(),
            Value::String("merchant".to_owned()),
        )])),
    )]);
    assert!(context_has_role(&actor_role, "merchant"));
}

#[test]
fn in_tenant_checks_structured_tenant_id() {
    let ctx = CoolContext::authenticated([(
        "tenant".to_owned(),
        Value::Map(BTreeMap::from([(
            "id".to_owned(),
            Value::String("tenant_1".to_owned()),
        )])),
    )]);
    assert!(context_in_tenant(&ctx, "tenant_1"));
    assert!(!context_in_tenant(&ctx, "tenant_2"));
}
