//! The create path is three-valued, literal side: `field <op> <literal>`,
//! `field in [..]`, `field not in [..]` and `auth().x <op> <literal>`. See
//! `create_deny` for the rule and the harness.

use cratestack_core::Value;

use super::create_deny::{allow_grants, authed, deny_fires, leaf, undecidable_claims};
use super::create_policies::{SET, amount, claim, eq_literal, ne_literal};
use crate::{PolicyLiteral, ReadPredicate, SqlValue};

/// The same rule for the comparisons decided against a literal.
#[tokio::test]
async fn a_deny_fires_for_a_literal_it_cannot_compare() {
    let plain = authed();
    let in_set = ReadPredicate::FieldInLiterals {
        column: "amount",
        values: &SET,
    };
    let not_in_set = ReadPredicate::FieldNotInLiterals {
        column: "amount",
        values: &SET,
    };
    // A NULL column, for every operator.
    let null = amount(SqlValue::NullBigInt);
    for deny in [eq_literal(7), ne_literal(7), in_set, not_in_set] {
        assert!(deny_fires(leaf(deny), &null, &plain).await, "{deny:?}");
    }
    // An integer column against a string literal.
    let text = PolicyLiteral::String("7");
    let big = amount(SqlValue::BigInt(7));
    for deny in [
        ReadPredicate::FieldEqLiteral {
            column: "amount",
            value: text,
        },
        ReadPredicate::FieldNeLiteral {
            column: "amount",
            value: text,
        },
    ] {
        assert!(deny_fires(leaf(deny), &big, &plain).await, "{deny:?}");
    }
    // `auth().tenant <op> 7` with a claim of the wrong type.
    for claim_value in undecidable_claims() {
        let ctx = claim(claim_value.clone());
        for deny in [
            ReadPredicate::AuthFieldEqLiteral {
                auth_field: "tenant",
                value: PolicyLiteral::Int(7),
            },
            ReadPredicate::AuthFieldNeLiteral {
                auth_field: "tenant",
                value: PolicyLiteral::Int(7),
            },
        ] {
            assert!(
                deny_fires(leaf(deny), &[], &ctx).await,
                "{deny:?} for the claim {claim_value:?}"
            );
        }
    }
}

/// Literal comparisons that are decided keep their answers under `@deny`.
#[tokio::test]
async fn a_decided_literal_comparison_denies_exactly_when_it_holds() {
    let plain = authed();
    let in_set = ReadPredicate::FieldInLiterals {
        column: "amount",
        values: &SET,
    };
    let not_in_set = ReadPredicate::FieldNotInLiterals {
        column: "amount",
        values: &SET,
    };
    let member = amount(SqlValue::BigInt(7));
    let outsider = amount(SqlValue::BigInt(8));
    assert!(deny_fires(leaf(eq_literal(7)), &member, &plain).await);
    assert!(!deny_fires(leaf(eq_literal(7)), &outsider, &plain).await);
    assert!(!deny_fires(leaf(ne_literal(7)), &member, &plain).await);
    assert!(deny_fires(leaf(ne_literal(7)), &outsider, &plain).await);
    assert!(deny_fires(leaf(in_set), &member, &plain).await);
    assert!(!deny_fires(leaf(in_set), &outsider, &plain).await);
    assert!(!deny_fires(leaf(not_in_set), &member, &plain).await);
    assert!(deny_fires(leaf(not_in_set), &outsider, &plain).await);

    let eq = ReadPredicate::AuthFieldEqLiteral {
        auth_field: "tenant",
        value: PolicyLiteral::Int(7),
    };
    assert!(deny_fires(leaf(eq), &[], &claim(Value::Int(7))).await);
    assert!(!deny_fires(leaf(eq), &[], &claim(Value::Int(8))).await);
}

/// `x not in [a, b]` is `x != a AND x != b`: an Unknown operand with no False
/// beside it leaves it Unknown, a member (False) decides it. `x in [a, b]` is
/// the disjunction, decided by a True.
#[tokio::test]
async fn in_and_not_in_follow_the_connectives_over_their_elements() {
    let plain = authed();
    // A set the column can only half-compare with: an integer and a string.
    static MIXED: [PolicyLiteral; 2] = [PolicyLiteral::Int(7), PolicyLiteral::String("x")];
    let in_mixed = ReadPredicate::FieldInLiterals {
        column: "amount",
        values: &MIXED,
    };
    let not_in_mixed = ReadPredicate::FieldNotInLiterals {
        column: "amount",
        values: &MIXED,
    };
    let seven = amount(SqlValue::BigInt(7));
    let eight = amount(SqlValue::BigInt(8));
    // 7 in [7, "x"] is True OR Unknown = True: the deny fires, the allow grants.
    assert!(deny_fires(leaf(in_mixed), &seven, &plain).await);
    assert!(allow_grants(leaf(in_mixed), &seven, &plain).await);
    // 8 in [7, "x"] is False OR Unknown = Unknown: the deny fires, no grant.
    assert!(deny_fires(leaf(in_mixed), &eight, &plain).await);
    assert!(!allow_grants(leaf(in_mixed), &eight, &plain).await);
    // 7 not in [7, "x"] is False AND Unknown = False: the deny stays silent.
    assert!(!deny_fires(leaf(not_in_mixed), &seven, &plain).await);
    assert!(!allow_grants(leaf(not_in_mixed), &seven, &plain).await);
    // 8 not in [7, "x"] is True AND Unknown = Unknown: the deny fires.
    assert!(deny_fires(leaf(not_in_mixed), &eight, &plain).await);
    assert!(!allow_grants(leaf(not_in_mixed), &eight, &plain).await);
}
