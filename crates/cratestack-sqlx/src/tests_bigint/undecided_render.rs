//! `auth().x <op> <literal>` is decided in-process and emitted as a constant.
//! A claim of the wrong type cannot be compared, and what that renders as is
//! what keeps a `@deny` honest: `FALSE` made `NOT (FALSE)` true, so the deny
//! never fired on read, update or delete. In a clause it is now `NULL`, which
//! SQL carries the way it carries any unknown (`NOT (NULL)` is NULL and
//! refuses the row; an `@allow` on NULL grants nothing). Inside a relation
//! quantifier it stays `FALSE`, because `every` is `NOT EXISTS (.. AND NOT
//! (expr))` and a NULL `expr` would read as "no counterexample".
//!
//! No database: the executed form (`push_action_policy_query`) and the preview
//! (`render_read_policy_sql`) must produce the same text. `pg::undecided` runs
//! the same policies against Postgres.

use cratestack_core::{CratestackContext, Value};

use super::create_policies::claim;
use crate::query::push_action_policy_query;
use crate::render::render_read_policy_sql;
use crate::{PolicyExpr, PolicyLiteral, ReadPolicy, ReadPredicate, RelationQuantifier, sqlx};

pub(super) fn eq_seven() -> ReadPredicate {
    ReadPredicate::AuthFieldEqLiteral {
        auth_field: "tenant",
        value: PolicyLiteral::Int(7),
    }
}

pub(super) fn ne_seven() -> ReadPredicate {
    ReadPredicate::AuthFieldNeLiteral {
        auth_field: "tenant",
        value: PolicyLiteral::Int(7),
    }
}

fn policy(expr: PolicyExpr) -> [ReadPolicy; 1] {
    [ReadPolicy { expr }]
}

fn leaf(predicate: ReadPredicate) -> PolicyExpr {
    PolicyExpr::Predicate(predicate)
}

fn executed(allow: &[ReadPolicy], deny: &[ReadPolicy], ctx: &CratestackContext) -> String {
    let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("");
    push_action_policy_query(&mut query, allow, deny, ctx);
    query.sql().as_str().to_owned()
}

fn rendered(allow: &[ReadPolicy], deny: &[ReadPolicy], ctx: &CratestackContext) -> String {
    let mut bind_index = 1usize;
    render_read_policy_sql(allow, deny, ctx, &mut bind_index).expect("renders")
}

fn assert_agree(
    allow: &[ReadPolicy],
    deny: &[ReadPolicy],
    ctx: &CratestackContext,
    expected: &str,
) {
    assert_eq!(executed(allow, deny, ctx), expected, "executed");
    assert_eq!(rendered(allow, deny, ctx), expected, "rendered");
}

/// Claims `auth().tenant <op> 7` cannot be decided with.
pub(super) fn undecidable_claims() -> Vec<Value> {
    vec![
        Value::String("7".to_owned()),
        Value::String("abc".to_owned()),
        Value::Bool(true),
        Value::Null,
        Value::Float(7.0),
    ]
}

/// Was `(NOT (FALSE) AND (TRUE))`: the deny never fired.
#[test]
fn a_deny_on_an_undecidable_claim_renders_null_so_the_row_is_refused() {
    let allow = policy(leaf(ReadPredicate::AuthNotNull));
    for deny in [eq_seven(), ne_seven()] {
        let deny = policy(leaf(deny));
        for undecidable in undecidable_claims() {
            assert_agree(
                &allow,
                &deny,
                &claim(undecidable.clone()),
                "(NOT (NULL) AND (TRUE))",
            );
        }
    }
}

/// An `@allow` on NULL grants nothing, as on `FALSE` before.
#[test]
fn an_allow_on_an_undecidable_claim_renders_null_and_grants_nothing() {
    for allow in [eq_seven(), ne_seven()] {
        let allow = policy(leaf(allow));
        for undecidable in undecidable_claims() {
            assert_agree(&allow, &[], &claim(undecidable), "(NULL)");
        }
    }
}

/// A claim that can be compared renders the constant it always did.
#[test]
fn a_decidable_claim_renders_true_or_false_as_before() {
    let allow_any = policy(leaf(ReadPredicate::AuthNotNull));
    for (predicate, claim_value, constant) in [
        (eq_seven(), 7, "TRUE"),
        (eq_seven(), 8, "FALSE"),
        (ne_seven(), 7, "FALSE"),
        (ne_seven(), 8, "TRUE"),
    ] {
        let ctx = claim(Value::Int(claim_value));
        assert_agree(
            &policy(leaf(predicate)),
            &[],
            &ctx,
            &format!("({constant})"),
        );
        assert_agree(
            &allow_any,
            &policy(leaf(predicate)),
            &ctx,
            &format!("(NOT ({constant}) AND (TRUE))"),
        );
    }
}

/// Not part of this change: a claim that is absent stays `FALSE` (a separate
/// decision), so a deny stays silent on it.
#[test]
fn an_absent_claim_stays_false() {
    let allow = policy(leaf(ReadPredicate::AuthNotNull));
    let authed_without = CratestackContext::authenticated(Vec::<(String, Value)>::new());
    for predicate in [eq_seven(), ne_seven()] {
        let deny = policy(leaf(predicate));
        assert_agree(&allow, &deny, &authed_without, "(NOT (FALSE) AND (TRUE))");
        assert_agree(&deny, &[], &authed_without, "(FALSE)");
    }
}

/// NULL travels through `and` / `or` like any unknown: `NULL AND TRUE` is
/// NULL, `NULL OR TRUE` is TRUE, and it is SQL that decides, not us.
#[test]
fn null_is_rendered_inside_and_or_groups() {
    static GROUP: [PolicyExpr; 2] = [
        PolicyExpr::Predicate(ReadPredicate::AuthNotNull),
        PolicyExpr::Predicate(ReadPredicate::AuthFieldEqLiteral {
            auth_field: "tenant",
            value: PolicyLiteral::Int(7),
        }),
    ];
    let ctx = claim(Value::String("7".to_owned()));
    assert_agree(
        &policy(PolicyExpr::And(&GROUP)),
        &[],
        &ctx,
        "((TRUE AND NULL))",
    );
    assert_agree(
        &policy(PolicyExpr::Or(&GROUP)),
        &[],
        &ctx,
        "((TRUE OR NULL))",
    );
}

fn relation(quantifier: RelationQuantifier, predicate: ReadPredicate) -> [ReadPolicy; 1] {
    let inner: &'static PolicyExpr = Box::leak(Box::new(leaf(predicate)));
    policy(leaf(ReadPredicate::Relation {
        quantifier,
        parent_table: "orders",
        parent_column: "id",
        related_table: "lines",
        related_column: "order_id",
        expr: inner,
    }))
}

/// Inside a relation quantifier the constant stays `FALSE`. `every` negates
/// its expression, so NULL there would be "no counterexample" and grant every
/// parent whose claim cannot be compared.
#[test]
fn inside_a_relation_an_undecidable_claim_stays_false() {
    let ctx = claim(Value::String("7".to_owned()));
    for predicate in [eq_seven(), ne_seven()] {
        for (quantifier, tail) in [
            (RelationQuantifier::Every, " AND NOT (FALSE)))"),
            (RelationQuantifier::Some, " AND FALSE))"),
            (RelationQuantifier::None, " AND FALSE))"),
            (RelationQuantifier::ToOne, " AND FALSE))"),
        ] {
            let allow = relation(quantifier, predicate);
            let sql = executed(&allow, &[], &ctx);
            assert!(sql.ends_with(tail), "{quantifier:?}: {sql}");
            assert!(!sql.contains("NULL"), "{quantifier:?}: {sql}");
            assert_agree(&allow, &[], &ctx, &sql);
        }
    }
}
