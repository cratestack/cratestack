//! `BigInt` in the dialect-agnostic value layer.

#![cfg(test)]

use cratestack_core::BigInt;

use super::{FilterValue, IntoSqlValue, SqlValue, find_duplicate_sql_value};
use crate::{CreateDefaultType, FieldRef, FilterOp};

const BOUNDARIES: [i64; 5] = [i64::MIN, -1, 0, 9_007_199_254_740_993, i64::MAX];

#[test]
fn a_bigint_lifts_to_the_bigint_variant_not_int() {
    for value in BOUNDARIES {
        assert_eq!(BigInt::new(value).into_sql_value(), SqlValue::BigInt(value));
        assert_ne!(BigInt::new(value).into_sql_value(), SqlValue::Int(value));
    }
}

#[test]
fn a_bigint_filter_carries_bigint_values() {
    let amount = FieldRef::<(), BigInt>::new("amount");
    let gt = amount.gt(BigInt::MAX);
    assert_eq!(gt.op, FilterOp::Gt);
    assert_eq!(gt.value, FilterValue::Single(SqlValue::BigInt(i64::MAX)));
    let within = amount.in_([BigInt::MIN, BigInt::new(7)]);
    assert_eq!(
        within.value,
        FilterValue::Many(vec![SqlValue::BigInt(i64::MIN), SqlValue::BigInt(7)])
    );
}

/// Batch-upsert deduplication compares one column's values, which are all the
/// same variant, so derived equality is the right test there.
#[test]
fn duplicate_detection_over_bigint_values() {
    let values = [
        SqlValue::BigInt(1),
        SqlValue::BigInt(i64::MAX),
        SqlValue::BigInt(1),
    ];
    assert_eq!(find_duplicate_sql_value(&values), Some((0, 2)));
    let distinct = [SqlValue::BigInt(7), SqlValue::NullBigInt, SqlValue::Int(7)];
    assert_eq!(find_duplicate_sql_value(&distinct), None);
}

#[test]
fn the_create_default_kinds_are_distinct() {
    assert_ne!(CreateDefaultType::BigInt, CreateDefaultType::Int);
}
