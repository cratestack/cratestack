//! Risk 2 on the create path, the claim side: `field != auth().x` and
//! `auth().x != <literal>`, and the whole create decision with allow and deny
//! lists.

use cratestack_core::{CratestackContext, Value};

use super::BOUNDARIES;
use super::create_policies::{EQ_AUTH, NE_AUTH, amount, claim, eq_literal, ne_literal};
use crate::query::evaluate_input_predicate_for_tests as evaluate;
use crate::{PolicyExpr, PolicyLiteral, ReadPolicy, ReadPredicate, SqlValue};

/// Was: `candidate != &auth_value` over derived `PartialEq`, where
/// `BigInt(7) != Int(7)` is `true`.
#[test]
fn field_ne_auth_compares_a_bigint_column_numerically_with_an_integer_claim() {
    for value in BOUNDARIES {
        let ctx = claim(Value::Int(value));
        let other = if value == 0 { 1 } else { 0 };
        assert!(!evaluate(NE_AUTH, &amount(SqlValue::BigInt(value)), &ctx));
        assert!(evaluate(EQ_AUTH, &amount(SqlValue::BigInt(value)), &ctx));
        assert!(evaluate(NE_AUTH, &amount(SqlValue::BigInt(other)), &ctx));
        assert!(!evaluate(EQ_AUTH, &amount(SqlValue::BigInt(other)), &ctx));
    }
}

/// A `BigInt` auth claim must be an integer in a policy comparison. A string
/// claim (canonical or not), a bool, or a NULL column satisfies neither `==`
/// nor `!=`; before, a `"7"` claim satisfied `!=`.
#[test]
fn a_non_integer_claim_or_a_null_column_satisfies_neither_side() {
    for text in ["7", "007", " 7", "+7", "-0", "7.0", ""] {
        let ctx = claim(Value::String(text.to_owned()));
        assert!(!evaluate(NE_AUTH, &amount(SqlValue::BigInt(7)), &ctx));
        assert!(!evaluate(EQ_AUTH, &amount(SqlValue::BigInt(7)), &ctx));
    }
    let ctx = claim(Value::Bool(true));
    assert!(!evaluate(NE_AUTH, &amount(SqlValue::BigInt(7)), &ctx));
    assert!(!evaluate(EQ_AUTH, &amount(SqlValue::BigInt(7)), &ctx));
    let ctx = claim(Value::Int(7));
    assert!(!evaluate(NE_AUTH, &amount(SqlValue::NullBigInt), &ctx));
    assert!(!evaluate(EQ_AUTH, &amount(SqlValue::NullBigInt), &ctx));
}

/// `auth().tenant != 7` is decided in-process at render time and has the same
/// two sides.
#[test]
fn auth_field_literal_predicates_compare_an_integer_claim_and_refuse_a_string() {
    let ne = ReadPredicate::AuthFieldNeLiteral {
        auth_field: "tenant",
        value: PolicyLiteral::Int(7),
    };
    let eq = ReadPredicate::AuthFieldEqLiteral {
        auth_field: "tenant",
        value: PolicyLiteral::Int(7),
    };
    assert!(!evaluate(ne, &[], &claim(Value::Int(7))));
    assert!(evaluate(eq, &[], &claim(Value::Int(7))));
    assert!(evaluate(ne, &[], &claim(Value::Int(8))));
    assert!(!evaluate(eq, &[], &claim(Value::Int(8))));
    for refused in [
        Value::String("7".to_owned()),
        Value::Null,
        Value::Bool(true),
    ] {
        assert!(!evaluate(ne, &[], &claim(refused.clone())));
        assert!(!evaluate(eq, &[], &claim(refused)));
    }
    assert!(!evaluate(ne, &[], &CratestackContext::anonymous()));
}

/// The `Int` pairs are unchanged: derived equality, so equal denies `!=`.
#[test]
fn int_against_int_is_unchanged() {
    let ctx = claim(Value::Int(7));
    assert!(!evaluate(NE_AUTH, &amount(SqlValue::Int(7)), &ctx));
    assert!(evaluate(EQ_AUTH, &amount(SqlValue::Int(7)), &ctx));
    assert!(evaluate(NE_AUTH, &amount(SqlValue::Int(8)), &ctx));
    let anon = CratestackContext::anonymous();
    assert!(!evaluate(ne_literal(7), &amount(SqlValue::Int(7)), &anon));
    assert!(evaluate(ne_literal(7), &amount(SqlValue::Int(8)), &anon));
}

/// The whole create decision, allow and deny lists included, with no database
/// (no relation predicate, so the pool is never used).
#[tokio::test]
async fn evaluate_create_policies_denies_a_negated_bigint_policy() {
    use crate::query::{PolicyDb, evaluate_create_policies};

    let pool = crate::sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://unused@127.0.0.1:1/none")
        .expect("a lazy pool does not connect");
    let ctx = claim(Value::Int(7));
    let allow_ne = [ReadPolicy {
        expr: PolicyExpr::Predicate(NE_AUTH),
    }];
    let allow_any = [ReadPolicy {
        expr: PolicyExpr::Predicate(ReadPredicate::AuthNotNull),
    }];
    let deny_eq = [ReadPolicy {
        expr: PolicyExpr::Predicate(eq_literal(7)),
    }];
    let decide = |allow: &'static [ReadPolicy], deny: &'static [ReadPolicy], value: i64| {
        let pool = pool.clone();
        let ctx = ctx.clone();
        async move {
            let values = amount(SqlValue::BigInt(value));
            evaluate_create_policies(PolicyDb::Pool(&pool), allow, deny, &values, &ctx)
                .await
                .expect("no relation predicate, so no query")
        }
    };
    let allow_ne: &'static [ReadPolicy] = Box::leak(Box::new(allow_ne));
    let allow_any: &'static [ReadPolicy] = Box::leak(Box::new(allow_any));
    let deny_eq: &'static [ReadPolicy] = Box::leak(Box::new(deny_eq));

    assert!(!decide(allow_ne, &[], 7).await, "`!= auth()` must deny 7");
    assert!(decide(allow_ne, &[], 8).await);
    assert!(!decide(allow_any, deny_eq, 7).await, "a deny rule wins");
    assert!(decide(allow_any, deny_eq, 8).await);
}
