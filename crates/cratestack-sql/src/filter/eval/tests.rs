use super::{in_memory_columns, like, matches};
use crate::filter::{Filter, FilterExpr, FilterOp};
use crate::values::FilterValue;
use crate::{SqlColumnValue, SqlValue};

fn filter(column: &'static str, op: FilterOp, value: FilterValue) -> FilterExpr {
    FilterExpr::Filter(Filter { column, op, value })
}

fn eq(column: &'static str, value: SqlValue) -> FilterExpr {
    filter(column, FilterOp::Eq, FilterValue::Single(value))
}

fn record() -> Vec<SqlColumnValue> {
    vec![
        SqlColumnValue {
            column: "status",
            value: SqlValue::String("open".into()),
        },
        SqlColumnValue {
            column: "amount",
            value: SqlValue::BigInt(42),
        },
        SqlColumnValue {
            column: "note",
            value: SqlValue::NullString,
        },
    ]
}

#[test]
fn comparisons_follow_sql() {
    let r = record();
    assert!(matches(&eq("status", SqlValue::String("open".into())), &r));
    assert!(!matches(
        &eq("status", SqlValue::String("closed".into())),
        &r
    ));
    // `BigInt` and `Int` compare numerically.
    assert!(matches(
        &filter(
            "amount",
            FilterOp::Gt,
            FilterValue::Single(SqlValue::Int(40))
        ),
        &r
    ));
    assert!(matches(
        &filter(
            "amount",
            FilterOp::In,
            FilterValue::Many(vec![SqlValue::Int(1), SqlValue::Int(42)])
        ),
        &r
    ));
}

#[test]
fn null_is_unknown_and_a_where_keeps_only_true() {
    let r = record();
    let note_is_x = eq("note", SqlValue::String("x".into()));
    assert!(!matches(&note_is_x, &r));
    // NOT (NULL = 'x') is still unknown, so the row is not kept either.
    assert!(!matches(&FilterExpr::Not(Box::new(note_is_x.clone())), &r));
    // Unknown OR true is true.
    assert!(matches(
        &FilterExpr::Any(vec![note_is_x, eq("amount", SqlValue::Int(42))]),
        &r
    ));
    assert!(matches(
        &filter("note", FilterOp::IsNull, FilterValue::None),
        &r
    ));
    assert!(matches(
        &filter(
            "note",
            FilterOp::EqOrNull,
            FilterValue::Single(SqlValue::String("y".into()))
        ),
        &r
    ));
}

#[test]
fn a_column_the_record_lacks_never_matches() {
    assert!(!matches(&eq("missing", SqlValue::Int(1)), &record()));
}

#[test]
fn like_patterns() {
    assert!(like("hello world", "%lo w%"));
    assert!(like("hello", "he%"));
    assert!(like("hello", "h_llo"));
    assert!(!like("hello", "h_lo"));
    assert!(like("", "%"));
    assert!(!like("abc", "abd%"));
}

#[test]
fn only_the_scalar_subset_is_decidable_in_memory() {
    let expr = FilterExpr::All(vec![
        eq("status", SqlValue::String("open".into())),
        FilterExpr::Not(Box::new(eq("amount", SqlValue::Int(1)))),
    ]);
    assert_eq!(in_memory_columns(&expr), Ok(vec!["amount", "status"]));
}
