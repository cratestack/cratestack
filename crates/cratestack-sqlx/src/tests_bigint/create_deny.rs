//! The create path is three-valued. An `@deny` fires on anything that is not
//! False, so a comparison that cannot be decided (a string claim against a
//! `BigInt` column, a NULL column, a string literal against an integer)
//! refuses the create instead of reading as "not equal"; an `@allow` still
//! needs a decided match. Read, update and delete get the same answer from
//! SQL's own `NOT (unknown)`, and `cratestack-policy` gives it to procedures
//! (`truth.rs`); this pins the in-process evaluator the create path uses.
//!
//! This file holds the harness and the claim comparisons
//! (`field <op> auth().x`); `create_deny_literals` has the literal ones and
//! `create_deny_groups` the `and` / `or` and list combinators.
//!
//! Every decision goes through `evaluate_create_policies`, so the allow and
//! deny lists and the `and`/`or` evaluator are exercised, not just the leaf
//! predicates. No relation predicate appears, so the lazy pool is never asked
//! for a connection.

use cratestack_core::{CratestackContext, Value};

use super::BOUNDARIES;
use super::create_policies::{EQ_AUTH, NE_AUTH, amount, claim, eq_literal, ne_literal};
use crate::query::{PolicyDb, evaluate_create_policies};
use crate::{PolicyExpr, ReadPolicy, ReadPredicate, SqlColumnValue, SqlValue};

pub(super) fn rule(expr: PolicyExpr) -> [ReadPolicy; 1] {
    [ReadPolicy { expr }]
}

pub(super) async fn decide(
    allow: &[ReadPolicy],
    deny: &[ReadPolicy],
    values: &[SqlColumnValue],
    ctx: &CratestackContext,
) -> bool {
    let pool = crate::sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://unused@127.0.0.1:1/none")
        .expect("a lazy pool does not connect");
    evaluate_create_policies(PolicyDb::Pool(&pool), allow, deny, values, ctx)
        .await
        .expect("no relation predicate, so no query")
}

/// Is a create refused by this one `@deny`, with a blanket `@allow` beside it?
pub(super) async fn deny_fires(
    deny: PolicyExpr,
    values: &[SqlColumnValue],
    ctx: &CratestackContext,
) -> bool {
    let allow = rule(PolicyExpr::Predicate(ReadPredicate::AuthNotNull));
    !decide(&allow, &rule(deny), values, ctx).await
}

/// Is a create granted by this one `@allow`, with no `@deny`?
pub(super) async fn allow_grants(
    allow: PolicyExpr,
    values: &[SqlColumnValue],
    ctx: &CratestackContext,
) -> bool {
    decide(&rule(allow), &[], values, ctx).await
}

pub(super) fn leaf(predicate: ReadPredicate) -> PolicyExpr {
    PolicyExpr::Predicate(predicate)
}

/// Authenticated, with a claim the comparison under test does not read. The
/// blanket `@allow` in [`deny_fires`] is `auth() != null`, so an anonymous
/// context would be refused by it and make every `@deny` test pass for the
/// wrong reason.
pub(super) fn authed() -> CratestackContext {
    claim(Value::Int(0))
}

/// Authenticated, with no `tenant` claim at all.
fn authed_without_a_claim() -> CratestackContext {
    CratestackContext::authenticated(Vec::<(String, Value)>::new())
}

/// Claims a `BigInt` column (or an integer literal) cannot be compared with.
pub(super) fn undecidable_claims() -> Vec<Value> {
    let mut claims = vec![Value::Bool(true), Value::Bool(false)];
    for text in ["7", "007", " 7", "+7", "-0", "7.0", "", "abc"] {
        claims.push(Value::String(text.to_owned()));
    }
    claims
}

/// The reported fail-open. Was: `is_equal()` / `is_different()` on an
/// undecidable pair were both false, so a `@deny` built on `==` or `!=` stayed
/// silent and the create went through.
#[tokio::test]
async fn a_deny_on_a_bigint_column_fires_for_a_claim_it_cannot_compare() {
    for claim_value in undecidable_claims() {
        let ctx = claim(claim_value.clone());
        for value in [7, 8] {
            let values = amount(SqlValue::BigInt(value));
            for (name, deny) in [("==", EQ_AUTH), ("!=", NE_AUTH)] {
                assert!(
                    deny_fires(leaf(deny), &values, &ctx).await,
                    "`deny amount {name} auth().tenant` must refuse BigInt({value}) \
                     for the claim {claim_value:?}"
                );
            }
        }
    }
}

/// An `@allow` needs a decided match, so an undecidable pair grants nothing,
/// whichever operator it uses. This held before the change and must keep
/// holding.
#[tokio::test]
async fn an_allow_never_grants_on_a_pair_it_cannot_compare() {
    for claim_value in undecidable_claims() {
        let ctx = claim(claim_value.clone());
        let values = amount(SqlValue::BigInt(7));
        for allow in [EQ_AUTH, NE_AUTH] {
            assert!(
                !allow_grants(leaf(allow), &values, &ctx).await,
                "{allow:?} for the claim {claim_value:?}"
            );
        }
    }
    let plain = authed();
    let null = amount(SqlValue::NullBigInt);
    for allow in [eq_literal(7), ne_literal(7)] {
        assert!(!allow_grants(leaf(allow), &null, &plain).await, "{allow:?}");
    }
}

/// A pair that can be compared is decided exactly as before, in both
/// directions, for `@deny` and for `@allow`.
#[tokio::test]
async fn an_integer_claim_decides_allow_and_deny_both_ways() {
    for value in BOUNDARIES {
        let other = if value == 0 { 1 } else { 0 };
        let ctx = claim(Value::Int(value));
        let equal = amount(SqlValue::BigInt(value));
        let unequal = amount(SqlValue::BigInt(other));

        assert!(deny_fires(leaf(EQ_AUTH), &equal, &ctx).await);
        assert!(!deny_fires(leaf(EQ_AUTH), &unequal, &ctx).await);
        assert!(!deny_fires(leaf(NE_AUTH), &equal, &ctx).await);
        assert!(deny_fires(leaf(NE_AUTH), &unequal, &ctx).await);

        assert!(allow_grants(leaf(EQ_AUTH), &equal, &ctx).await);
        assert!(!allow_grants(leaf(EQ_AUTH), &unequal, &ctx).await);
        assert!(!allow_grants(leaf(NE_AUTH), &equal, &ctx).await);
        assert!(allow_grants(leaf(NE_AUTH), &unequal, &ctx).await);
    }
}

/// Pairs that were never undecidable keep their answers: `Int` against `Int`
/// and `String` against `String` are decided by derived equality, as before.
#[tokio::test]
async fn pairs_that_are_never_undecidable_keep_their_outcome() {
    let ctx = claim(Value::Int(7));
    let equal = amount(SqlValue::Int(7));
    let unequal = amount(SqlValue::Int(8));
    assert!(deny_fires(leaf(EQ_AUTH), &equal, &ctx).await);
    assert!(!deny_fires(leaf(EQ_AUTH), &unequal, &ctx).await);
    assert!(!deny_fires(leaf(NE_AUTH), &equal, &ctx).await);
    assert!(deny_fires(leaf(NE_AUTH), &unequal, &ctx).await);

    let ctx = claim(Value::String("acme".to_owned()));
    let equal = amount(SqlValue::String("acme".to_owned()));
    let unequal = amount(SqlValue::String("other".to_owned()));
    assert!(deny_fires(leaf(EQ_AUTH), &equal, &ctx).await);
    assert!(!deny_fires(leaf(EQ_AUTH), &unequal, &ctx).await);
    assert!(!deny_fires(leaf(NE_AUTH), &equal, &ctx).await);
    assert!(deny_fires(leaf(NE_AUTH), &unequal, &ctx).await);
}

/// Not part of this change, and pinned so a later one is deliberate: an
/// operand that is absent (no such column in the input, no such claim) or that
/// is a claim lowering to nothing (`Null`, `Float`) makes a column comparison
/// False, as the pushed-down form's `FALSE` constant does. Whether a `@deny`
/// should fire on those is a separate decision.
#[tokio::test]
async fn absent_and_unbindable_operands_are_unchanged() {
    let plain = authed_without_a_claim();
    let big = amount(SqlValue::BigInt(7));
    assert!(!deny_fires(leaf(EQ_AUTH), &big, &plain).await);
    assert!(!deny_fires(leaf(NE_AUTH), &big, &plain).await);
    assert!(!deny_fires(leaf(ne_literal(7)), &[], &plain).await);
    assert!(!deny_fires(leaf(eq_literal(7)), &[], &plain).await);
    for unbindable in [Value::Null, Value::Float(7.0)] {
        let ctx = claim(unbindable.clone());
        assert!(
            !deny_fires(leaf(NE_AUTH), &big, &ctx).await,
            "{unbindable:?}"
        );
        assert!(
            !deny_fires(leaf(EQ_AUTH), &big, &ctx).await,
            "{unbindable:?}"
        );
    }
}
