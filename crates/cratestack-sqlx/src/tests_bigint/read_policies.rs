//! Read, update and delete policies are pushed into SQL, where Postgres
//! decides `col != $1` itself. What this crate decides is the text and which
//! claims bind, so these pin that the executed form and the preview renderer
//! (`render_read_policy_sql`, which must stay in lock-step with it) agree for
//! a `BigInt`, and that an `auth().x <op> <literal>` predicate, which is
//! decided in-process and emitted as a `TRUE`/`FALSE` constant, refuses a
//! string claim on both sides. `pg` runs the same fragments against Postgres.

use cratestack_core::{CratestackContext, Value};

use crate::query::push_action_policy_query;
use crate::render::render_read_policy_sql;
use crate::{PolicyExpr, PolicyLiteral, ReadPolicy, ReadPredicate, sqlx};

static SET: [PolicyLiteral; 2] = [PolicyLiteral::Int(7), PolicyLiteral::Int(i64::MAX)];

fn policy(predicate: ReadPredicate) -> [ReadPolicy; 1] {
    [ReadPolicy {
        expr: PolicyExpr::Predicate(predicate),
    }]
}

fn claim(value: Value) -> CratestackContext {
    CratestackContext::authenticated([("tenant".to_owned(), value)])
}

fn executed(allow: &[ReadPolicy], ctx: &CratestackContext) -> String {
    let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("");
    push_action_policy_query(&mut query, allow, &[], ctx);
    query.sql().as_str().to_owned()
}

fn rendered(allow: &[ReadPolicy], ctx: &CratestackContext) -> String {
    let mut bind_index = 1usize;
    render_read_policy_sql(allow, &[], ctx, &mut bind_index).expect("renders")
}

fn assert_agree(predicate: ReadPredicate, ctx: &CratestackContext, expected: &str) {
    let allow = policy(predicate);
    assert_eq!(executed(&allow, ctx), expected, "executed {predicate:?}");
    assert_eq!(rendered(&allow, ctx), expected, "rendered {predicate:?}");
}

#[test]
fn negated_bigint_predicates_push_a_column_comparison_not_a_constant() {
    let anon = CratestackContext::anonymous();
    assert_agree(
        ReadPredicate::FieldNeLiteral {
            column: "amount",
            value: PolicyLiteral::Int(7),
        },
        &anon,
        "(amount != $1)",
    );
    assert_agree(
        ReadPredicate::FieldNotInLiterals {
            column: "amount",
            values: &SET,
        },
        &anon,
        "(amount NOT IN ($1, $2))",
    );
    assert_agree(
        ReadPredicate::FieldNeAuth {
            column: "amount",
            auth_field: "tenant",
        },
        &claim(Value::Int(7)),
        "(amount != $1)",
    );
}

/// A string claim still emits a comparison (the column type is unknown here);
/// `pg` shows Postgres refuses it. A claim of a type no `SqlValue` carries
/// emits `FALSE`, on both sides.
#[test]
fn an_unbindable_claim_denies_a_field_auth_comparison() {
    for predicate in [
        ReadPredicate::FieldNeAuth {
            column: "amount",
            auth_field: "tenant",
        },
        ReadPredicate::FieldEqAuth {
            column: "amount",
            auth_field: "tenant",
        },
    ] {
        assert_agree(predicate, &claim(Value::Null), "(FALSE)");
        assert_agree(predicate, &CratestackContext::anonymous(), "(FALSE)");
    }
}

#[test]
fn auth_field_literal_predicates_agree_and_refuse_a_string_claim() {
    let ne = ReadPredicate::AuthFieldNeLiteral {
        auth_field: "tenant",
        value: PolicyLiteral::Int(7),
    };
    let eq = ReadPredicate::AuthFieldEqLiteral {
        auth_field: "tenant",
        value: PolicyLiteral::Int(7),
    };
    assert_agree(ne, &claim(Value::Int(8)), "(TRUE)");
    assert_agree(ne, &claim(Value::Int(7)), "(FALSE)");
    assert_agree(eq, &claim(Value::Int(7)), "(TRUE)");
    // Was `(TRUE)` for `ne`: a string never equalled the integer, so it
    // "differed".
    for refused in [
        Value::String("7".to_owned()),
        Value::Bool(true),
        Value::Null,
    ] {
        assert_agree(ne, &claim(refused.clone()), "(FALSE)");
        assert_agree(eq, &claim(refused), "(FALSE)");
    }
}

/// sqlx caches a prepared statement by SQL text alone, so a claim whose type
/// varies per request must name its type in the text or an integer claim and a
/// string claim would share one statement (see `claim_type_suffix` and
/// `pg::refuse`). The executed form and the preview renderer both do.
#[test]
fn a_string_claim_names_its_type_in_the_sql_and_an_integer_claim_does_not() {
    for (predicate, op) in [
        (
            ReadPredicate::FieldNeAuth {
                column: "amount",
                auth_field: "tenant",
            },
            "!=",
        ),
        (
            ReadPredicate::FieldEqAuth {
                column: "amount",
                auth_field: "tenant",
            },
            "=",
        ),
    ] {
        assert_agree(
            predicate,
            &claim(Value::String("7".to_owned())),
            &format!("(amount {op} $1::text)"),
        );
        assert_agree(
            predicate,
            &claim(Value::Int(7)),
            &format!("(amount {op} $1)"),
        );
    }
}
