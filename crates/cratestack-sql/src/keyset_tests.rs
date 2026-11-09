use cratestack_core::CratestackError;

use super::*;
use crate::{FieldRef, FilterExpr, SqlColumnValue, SqlValue, filter_matches};

fn nullable_of(name: &str) -> Option<bool> {
    match name {
        "id" | "createdAt" => Some(false),
        "score" => Some(true),
        _ => None,
    }
}

#[test]
fn sort_parses_with_primary_key_tiebreaker_and_canonical_spec() {
    let cols = parse_keyset_sort(Some("-createdAt, score"), "id", nullable_of).unwrap();
    assert_eq!(keyset_order_spec(&cols), "-createdAt,score,id");
    assert_eq!(
        cols.iter().map(|c| (c.descending, c.nullable)).collect::<Vec<_>>(),
        vec![(true, false), (false, true), (false, false)]
    );
    let default = parse_keyset_sort(None, "id", nullable_of).unwrap();
    assert_eq!(keyset_order_spec(&default), "id");
    // An explicit primary key is not duplicated.
    let explicit = parse_keyset_sort(Some("-id"), "id", nullable_of).unwrap();
    assert_eq!(keyset_order_spec(&explicit), "-id");
}

#[test]
fn sort_rejects_relation_paths_unknown_and_duplicate_columns() {
    for sort in ["author.name", "nope", "score,score", "score,,id"] {
        assert!(
            matches!(parse_keyset_sort(Some(sort), "id", nullable_of), Err(CratestackError::BadRequest(_))),
            "{sort}"
        );
    }
}

#[derive(Clone, Debug)]
struct Row {
    created_at: i64,
    score: Option<i64>,
    id: i64,
}

fn leaf(column: &str, leaf: KeysetLeaf<'_, String>) -> Result<FilterExpr, CratestackError> {
    let num = |v: &String| v.parse::<i64>().unwrap();
    Ok(match (column, leaf) {
        ("createdAt", KeysetLeaf::Eq(v)) => FieldRef::<(), i64>::new("created_at").eq(num(v)).into(),
        ("createdAt", KeysetLeaf::Gt(v)) => FieldRef::<(), i64>::new("created_at").gt(num(v)).into(),
        ("createdAt", KeysetLeaf::Lt(v)) => FieldRef::<(), i64>::new("created_at").lt(num(v)).into(),
        ("score", KeysetLeaf::Eq(v)) => FieldRef::<(), Option<i64>>::new("score").eq(num(v)).into(),
        ("score", KeysetLeaf::Gt(v)) => FieldRef::<(), Option<i64>>::new("score").gt(num(v)).into(),
        ("score", KeysetLeaf::Lt(v)) => FieldRef::<(), Option<i64>>::new("score").lt(num(v)).into(),
        ("score", KeysetLeaf::IsNull) => FieldRef::<(), Option<i64>>::new("score").is_null().into(),
        ("id", KeysetLeaf::Eq(v)) => FieldRef::<(), i64>::new("id").eq(num(v)).into(),
        ("id", KeysetLeaf::Gt(v)) => FieldRef::<(), i64>::new("id").gt(num(v)).into(),
        ("id", KeysetLeaf::Lt(v)) => FieldRef::<(), i64>::new("id").lt(num(v)).into(),
        other => panic!("unexpected leaf {other:?}"),
    })
}

fn cols_of(row: &Row) -> Vec<SqlColumnValue> {
    vec![
        SqlColumnValue { column: "created_at", value: SqlValue::Int(row.created_at) },
        SqlColumnValue { column: "score", value: row.score.map_or(SqlValue::NullInt, SqlValue::Int) },
        SqlColumnValue { column: "id", value: SqlValue::Int(row.id) },
    ]
}

/// Reference ordering: `-createdAt, score ASC NULLS LAST, id`.
fn expected_order(a: &Row, b: &Row) -> std::cmp::Ordering {
    use std::cmp::Ordering::*;
    b.created_at
        .cmp(&a.created_at)
        .then_with(|| match (a.score, b.score) {
            (Some(x), Some(y)) => x.cmp(&y),
            (Some(_), None) => Less,
            (None, Some(_)) => Greater,
            (None, None) => Equal,
        })
        .then(a.id.cmp(&b.id))
}

#[test]
fn keyset_predicate_selects_exactly_the_rows_after_the_cursor_row() {
    let cols = parse_keyset_sort(Some("-createdAt,score"), "id", nullable_of).unwrap();
    let mut rows = Vec::new();
    let mut id = 0;
    for created_at in [1, 2, 3] {
        for score in [Some(1), Some(2), None, Some(1)] {
            id += 1;
            rows.push(Row { created_at, score, id });
        }
    }
    rows.sort_by(expected_order);
    for (index, pivot) in rows.iter().enumerate() {
        let values = vec![
            Some(pivot.created_at.to_string()),
            pivot.score.map(|s| s.to_string()),
            Some(pivot.id.to_string()),
        ];
        let filter = keyset_after(&cols, &values, leaf).unwrap();
        let got: Vec<i64> = rows.iter().filter(|r| filter_matches(&filter, &cols_of(r))).map(|r| r.id).collect();
        let want: Vec<i64> = rows[index + 1..].iter().map(|r| r.id).collect();
        assert_eq!(got, want, "after {pivot:?}");
    }
}

#[test]
fn keyset_rejects_value_count_mismatch() {
    let cols = parse_keyset_sort(None, "id", nullable_of).unwrap();
    let none: [Option<String>; 0] = [];
    assert!(keyset_after(&cols, &none, leaf).is_err());
}
