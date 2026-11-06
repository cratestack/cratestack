use cratestack_core::{CratestackContext, Value};

use super::may_read_some_row;
use crate::{PolicyExpr, ReadPolicy, ReadPredicate};

const SIGNED_IN: ReadPolicy = ReadPolicy {
    expr: PolicyExpr::Predicate(ReadPredicate::AuthNotNull),
};
const OWNER_IS_CALLER: ReadPolicy = ReadPolicy {
    expr: PolicyExpr::Predicate(ReadPredicate::FieldEqAuth {
        column: "owner_id",
        auth_field: "id",
    }),
};
const ARCHIVED: ReadPolicy = ReadPolicy {
    expr: PolicyExpr::Predicate(ReadPredicate::FieldIsTrue { column: "archived" }),
};
const ADMIN: ReadPolicy = ReadPolicy {
    expr: PolicyExpr::Predicate(ReadPredicate::HasRole { role: "admin" }),
};

fn caller(id: i64) -> CratestackContext {
    CratestackContext::authenticated([("id".to_owned(), Value::Int(id))])
}

#[test]
fn a_caller_only_policy_is_decided_at_open() {
    let anonymous = CratestackContext::anonymous();
    assert!(!may_read_some_row(&[SIGNED_IN], &[], &anonymous));
    assert!(may_read_some_row(&[SIGNED_IN], &[], &caller(1)));
}

#[test]
fn a_comparison_with_a_missing_claim_can_never_pass() {
    // `owner_id = auth().id` renders as FALSE without the claim.
    assert!(!may_read_some_row(
        &[OWNER_IS_CALLER],
        &[],
        &CratestackContext::anonymous()
    ));
    assert!(may_read_some_row(&[OWNER_IS_CALLER], &[], &caller(1)));
}

#[test]
fn a_row_dependent_deny_never_refuses_at_open() {
    assert!(may_read_some_row(&[SIGNED_IN], &[ARCHIVED], &caller(1)));
}

#[test]
fn a_deny_true_for_the_caller_refuses() {
    let deny_non_admin = [ReadPolicy {
        expr: PolicyExpr::Or(&[PolicyExpr::Predicate(ReadPredicate::AuthNotNull)]),
    }];
    assert!(!may_read_some_row(
        &[SIGNED_IN],
        &deny_non_admin,
        &caller(1)
    ));
}

#[test]
fn combinators_keep_every_reachable_value() {
    static EITHER: [PolicyExpr; 2] = [
        PolicyExpr::Predicate(ReadPredicate::HasRole { role: "admin" }),
        PolicyExpr::Predicate(ReadPredicate::FieldEqAuth {
            column: "owner_id",
            auth_field: "id",
        }),
    ];
    let allow = [ReadPolicy {
        expr: PolicyExpr::Or(&EITHER),
    }];
    // Not an admin, but an owner of something.
    assert!(may_read_some_row(&allow, &[], &caller(7)));
    // Neither: anonymous has no role and no id.
    assert!(!may_read_some_row(
        &allow,
        &[],
        &CratestackContext::anonymous()
    ));
    assert!(!may_read_some_row(&[ADMIN], &[], &caller(7)));
    assert!(
        !may_read_some_row(&[], &[], &caller(7)),
        "no allow, no read"
    );
}
