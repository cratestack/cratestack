//! Risk 2 on the create path: a negated predicate over a `BigInt` column must
//! DENY the matching value. Each test names the construct that used to let it
//! through, so a regression reads as the bug it is.

use cratestack_core::{CratestackContext, Value};

use super::BOUNDARIES;
use crate::query::evaluate_input_predicate_for_tests as evaluate;
use crate::{PolicyLiteral, ReadPredicate, SqlColumnValue, SqlValue};

pub(super) fn amount(value: SqlValue) -> Vec<SqlColumnValue> {
    vec![SqlColumnValue {
        column: "amount",
        value,
    }]
}

pub(super) fn claim(value: Value) -> CratestackContext {
    CratestackContext::authenticated([("tenant".to_owned(), value)])
}

pub(super) fn ne_literal(value: i64) -> ReadPredicate {
    ReadPredicate::FieldNeLiteral {
        column: "amount",
        value: PolicyLiteral::Int(value),
    }
}

pub(super) fn eq_literal(value: i64) -> ReadPredicate {
    ReadPredicate::FieldEqLiteral {
        column: "amount",
        value: PolicyLiteral::Int(value),
    }
}

pub(super) static SET: [PolicyLiteral; 3] = [
    PolicyLiteral::Int(1),
    PolicyLiteral::Int(7),
    PolicyLiteral::Int(i64::MAX),
];

pub(super) const NE_AUTH: ReadPredicate = ReadPredicate::FieldNeAuth {
    column: "amount",
    auth_field: "tenant",
};
pub(super) const EQ_AUTH: ReadPredicate = ReadPredicate::FieldEqAuth {
    column: "amount",
    auth_field: "tenant",
};

/// Was: `BigInt` fell to `_ => false` in `sql_value_matches_literal`, so
/// `!matches` was `true` and `amount != 7` admitted `amount = 7`.
#[test]
fn field_ne_literal_denies_the_equal_value_and_admits_the_rest() {
    let anon = CratestackContext::anonymous();
    for value in BOUNDARIES {
        let other = if value == 0 { 1 } else { 0 };
        assert!(
            !evaluate(ne_literal(value), &amount(SqlValue::BigInt(value)), &anon),
            "`!= {value}` must deny BigInt({value})"
        );
        assert!(evaluate(
            ne_literal(value),
            &amount(SqlValue::BigInt(other)),
            &anon
        ));
        assert!(evaluate(
            eq_literal(value),
            &amount(SqlValue::BigInt(value)),
            &anon
        ));
        assert!(!evaluate(
            eq_literal(value),
            &amount(SqlValue::BigInt(other)),
            &anon
        ));
    }
}

/// Was: the same fallthrough through `!any(matches)`.
#[test]
fn field_not_in_literals_denies_a_member_and_admits_a_non_member() {
    let anon = CratestackContext::anonymous();
    let not_in = ReadPredicate::FieldNotInLiterals {
        column: "amount",
        values: &SET,
    };
    let is_in = ReadPredicate::FieldInLiterals {
        column: "amount",
        values: &SET,
    };
    for member in [1, 7, i64::MAX] {
        assert!(!evaluate(not_in, &amount(SqlValue::BigInt(member)), &anon));
        assert!(evaluate(is_in, &amount(SqlValue::BigInt(member)), &anon));
    }
    for outsider in [i64::MIN, 0, 8, 9_007_199_254_740_993] {
        assert!(evaluate(not_in, &amount(SqlValue::BigInt(outsider)), &anon));
        assert!(!evaluate(is_in, &amount(SqlValue::BigInt(outsider)), &anon));
    }
}

/// A NULL `BigInt`, and a `BigInt` against a literal of another kind, are not
/// comparable; they satisfy neither `==` nor `!=`, as SQL's own `NULL != 7`
/// does for the pushed-down form.
#[test]
fn an_undecidable_literal_comparison_denies_both_sides() {
    let anon = CratestackContext::anonymous();
    let not_in = ReadPredicate::FieldNotInLiterals {
        column: "amount",
        values: &SET,
    };
    let is_in = ReadPredicate::FieldInLiterals {
        column: "amount",
        values: &SET,
    };
    let null = amount(SqlValue::NullBigInt);
    for predicate in [ne_literal(7), eq_literal(7), not_in, is_in] {
        assert!(!evaluate(predicate, &null, &anon), "{predicate:?}");
    }

    let text = PolicyLiteral::String("7");
    let big = amount(SqlValue::BigInt(7));
    for predicate in [
        ReadPredicate::FieldNeLiteral {
            column: "amount",
            value: text,
        },
        ReadPredicate::FieldEqLiteral {
            column: "amount",
            value: text,
        },
    ] {
        assert!(!evaluate(predicate, &big, &anon), "{predicate:?}");
    }
    // A column the input does not carry fails both, as before.
    assert!(!evaluate(ne_literal(7), &[], &anon));
}
