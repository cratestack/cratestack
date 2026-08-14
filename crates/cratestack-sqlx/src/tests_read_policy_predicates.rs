#![cfg(test)]

//! No-database coverage of read-policy evaluation semantics
//! (`cratestack_policy::{ReadPolicy, PolicyExpr, ReadPredicate}`).
//! `cratestack-policy` itself only defines these as data — there is no
//! runtime evaluator in that crate to unit test directly, unlike
//! `authorize_procedure` for procedure policies. The actual evaluator
//! is `render_read_policy_sql` here, which compiles a policy straight
//! to a SQL predicate rather than evaluating it against an in-process
//! row. These tests exercise that compilation directly — no DB
//! connection needed, since it's pure string generation — for the
//! caller-context-only `ReadPredicate` variants, plus the default-deny
//! / deny-beats-allow precedence rules from `render_read_policy_sql`
//! itself. See `tests_read_policy_field_predicates.rs` for the
//! per-row-column and relation-quantifier variants, and
//! `tests_relation.rs` / `tests_nested_relation_policy.rs` for the
//! pre-existing coverage this extends (`FieldEqAuth`, `FieldEqLiteral`,
//! nested `ToOne`/`Every` relations).

use crate::{PolicyExpr, PolicyLiteral, ReadPolicy, ReadPredicate, render::render_read_policy_sql};
use cratestack_core::{CratestackContext, Value};

fn render(allow: &[ReadPolicy], deny: &[ReadPolicy], ctx: &CratestackContext) -> Option<String> {
    let mut bind_index = 1usize;
    render_read_policy_sql(allow, deny, ctx, &mut bind_index)
}

/// An empty `allow` list must deny every caller unconditionally —
/// mirrors `authorize_procedure`'s empty-`allow_policies` rule in
/// `cratestack-policy`, just compiled to SQL instead of evaluated
/// in-process.
#[test]
fn empty_allow_list_default_denies_with_sql_false() {
    let ctx = CratestackContext::anonymous();
    let sql = render(&[], &[], &ctx).expect("empty allow should still render a clause");
    assert_eq!(sql, "(FALSE)");

    // Also true for an authenticated caller — default-deny is
    // unconditional, not merely "unauthenticated is denied".
    let authenticated = CratestackContext::authenticated([]);
    let sql = render(&[], &[], &authenticated).expect("empty allow should still render a clause");
    assert_eq!(sql, "(FALSE)");
}

/// A matching `@@deny` must veto an otherwise-matching `@@allow`:
/// `NOT (deny) AND (allow)`, not just OR'd together.
#[test]
fn deny_beats_allow_precedence() {
    let ctx = CratestackContext::authenticated([]);
    let allow = [ReadPolicy {
        expr: PolicyExpr::Predicate(ReadPredicate::AuthNotNull),
    }];
    let deny = [ReadPolicy {
        expr: PolicyExpr::Predicate(ReadPredicate::AuthNotNull),
    }];

    let sql = render(&allow, &deny, &ctx).expect("policy should render");
    assert_eq!(sql, "(NOT (TRUE) AND (TRUE))");
}

#[test]
fn auth_not_null_and_auth_is_null_collapse_to_sql_booleans() {
    let authenticated = CratestackContext::authenticated([]);
    let anonymous = CratestackContext::anonymous();
    let allow_not_null = [ReadPolicy {
        expr: PolicyExpr::Predicate(ReadPredicate::AuthNotNull),
    }];
    let allow_is_null = [ReadPolicy {
        expr: PolicyExpr::Predicate(ReadPredicate::AuthIsNull),
    }];

    assert_eq!(
        render(&allow_not_null, &[], &authenticated).unwrap(),
        "(TRUE)"
    );
    assert_eq!(render(&allow_not_null, &[], &anonymous).unwrap(), "(FALSE)");
    assert_eq!(
        render(&allow_is_null, &[], &authenticated).unwrap(),
        "(FALSE)"
    );
    assert_eq!(render(&allow_is_null, &[], &anonymous).unwrap(), "(TRUE)");
}

#[test]
fn has_role_and_in_tenant_collapse_to_sql_booleans() {
    let admin =
        CratestackContext::authenticated([("role".to_owned(), Value::String("admin".to_owned()))]);
    let member =
        CratestackContext::authenticated([("role".to_owned(), Value::String("member".to_owned()))]);
    let allow_role = [ReadPolicy {
        expr: PolicyExpr::Predicate(ReadPredicate::HasRole { role: "admin" }),
    }];
    assert_eq!(render(&allow_role, &[], &admin).unwrap(), "(TRUE)");
    assert_eq!(render(&allow_role, &[], &member).unwrap(), "(FALSE)");

    let tenant_a = CratestackContext::authenticated([(
        "tenant".to_owned(),
        Value::Map(std::collections::BTreeMap::from([(
            "id".to_owned(),
            Value::String("tenant_a".to_owned()),
        )])),
    )]);
    let allow_tenant = [ReadPolicy {
        expr: PolicyExpr::Predicate(ReadPredicate::InTenant {
            tenant_id: "tenant_a",
        }),
    }];
    assert_eq!(render(&allow_tenant, &[], &tenant_a).unwrap(), "(TRUE)");
    assert_eq!(render(&allow_tenant, &[], &member).unwrap(), "(FALSE)");
}

#[test]
fn auth_field_eq_and_ne_literal_collapse_to_sql_booleans() {
    let banned = CratestackContext::authenticated([(
        "status".to_owned(),
        Value::String("banned".to_owned()),
    )]);
    let active = CratestackContext::authenticated([(
        "status".to_owned(),
        Value::String("active".to_owned()),
    )]);
    let allow_eq = [ReadPolicy {
        expr: PolicyExpr::Predicate(ReadPredicate::AuthFieldEqLiteral {
            auth_field: "status",
            value: PolicyLiteral::String("banned"),
        }),
    }];
    let allow_ne = [ReadPolicy {
        expr: PolicyExpr::Predicate(ReadPredicate::AuthFieldNeLiteral {
            auth_field: "status",
            value: PolicyLiteral::String("banned"),
        }),
    }];

    assert_eq!(render(&allow_eq, &[], &banned).unwrap(), "(TRUE)");
    assert_eq!(render(&allow_eq, &[], &active).unwrap(), "(FALSE)");
    assert_eq!(render(&allow_ne, &[], &banned).unwrap(), "(FALSE)");
    assert_eq!(render(&allow_ne, &[], &active).unwrap(), "(TRUE)");
}
