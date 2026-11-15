//! The analysis restates the evaluator's behaviour for an anonymous caller.
//! This pins each row of that table to `authorize_procedure` itself, so a
//! change to a predicate's anonymous behaviour cannot leave
//! `requires_principal` quietly wrong. Every input is chosen to satisfy the
//! term, so only the caller decides the outcome.

use cratestack_core::{CratestackContext, Value};

use super::{PolicyDialect, analyse_op_policy};
use crate::{
    ProcedureArgs, ProcedurePolicy, ProcedurePolicyExpr, ProcedurePolicyLiteral as Lit,
    ProcedurePredicate as P, authorize_procedure,
};

struct Args;
impl ProcedureArgs for Args {
    fn procedure_arg_value(&self, field: &str) -> Option<Value> {
        match field {
            "flag" => Some(Value::Bool(true)),
            "x" => Some(Value::Int(1)),
            "y" => Some(Value::Int(1)),
            "z" => Some(Value::Int(2)),
            "owner" => Some(Value::String("u1".into())),
            _ => None,
        }
    }
}

fn anonymous_passes(predicate: P) -> bool {
    let allow = [ProcedurePolicy {
        expr: ProcedurePolicyExpr::Predicate(predicate),
    }];
    authorize_procedure(&allow, &[], &Args, &CratestackContext::anonymous()).is_ok()
}

#[test]
fn the_verdict_for_each_term_matches_what_the_evaluator_does_for_an_anonymous_caller() {
    let rows: [(&str, P); 15] = [
        ("true", P::Literal(true)),
        ("false", P::Literal(false)),
        ("auth() != null", P::AuthNotNull),
        ("auth() == null", P::AuthIsNull),
        ("auth().isSystem()", P::AuthIsSystem),
        (r#"hasRole("a")"#, P::HasRole { role: "a" }),
        (r#"inTenant("t")"#, P::InTenant { tenant_id: "t" }),
        (
            "auth().tier == 1",
            P::AuthFieldEqLiteral {
                auth_field: "tier",
                value: Lit::Int(1),
            },
        ),
        (
            "auth().tier != 1",
            P::AuthFieldNeLiteral {
                auth_field: "tier",
                value: Lit::Int(1),
            },
        ),
        ("flag", P::InputFieldIsTrue { field: "flag" }),
        (
            "x == 1",
            P::InputFieldEqLiteral {
                field: "x",
                value: Lit::Int(1),
            },
        ),
        (
            "x != 2",
            P::InputFieldNeLiteral {
                field: "x",
                value: Lit::Int(2),
            },
        ),
        (
            "owner == auth().id",
            P::InputFieldEqAuth {
                field: "owner",
                auth_field: "id",
            },
        ),
        (
            "x == y",
            P::InputFieldEqInput {
                field: "x",
                other_field: "y",
            },
        ),
        (
            "x != z",
            P::InputFieldNeInput {
                field: "x",
                other_field: "z",
            },
        ),
    ];
    for (term, predicate) in rows {
        let verdict = analyse_op_policy(PolicyDialect::Procedure, &[term], &[]);
        assert!(verdict.unanalysed.is_empty(), "{term}");
        assert_eq!(
            verdict.requires_principal,
            !anonymous_passes(predicate),
            "`{term}` disagrees with the evaluator"
        );
    }
}

#[test]
fn a_context_with_a_principal_satisfies_what_requires_one() {
    let ctx = CratestackContext::authenticated([("id".to_owned(), Value::String("u1".into()))]);
    let allow = [ProcedurePolicy {
        expr: ProcedurePolicyExpr::Predicate(P::AuthNotNull),
    }];
    assert!(authorize_procedure(&allow, &[], &Args, &ctx).is_ok());
}
