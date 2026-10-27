//! What a snapshot written by an older cratestack reads back as.

use super::{ColumnDefault, GeneratedDefault};
use crate::ir::{Column, ColumnArity, ColumnType};

/// Write a column the way an old snapshot stored it, then read it back.
fn round_trip(stored: ColumnDefault) -> Option<ColumnDefault> {
    let column = Column {
        name: "c".to_owned(),
        ty: ColumnType::Scalar("String".to_owned()),
        arity: ColumnArity::Required,
        default: Some(stored),
        primary_key: false,
    };
    let json = serde_json::to_string(&column).expect("serialize");
    serde_json::from_str::<Column>(&json)
        .expect("deserialize")
        .default
}

#[test]
fn old_builtin_function_defaults_read_as_generated() {
    for (call, generated) in [
        ("now()", GeneratedDefault::Now),
        ("uuid()", GeneratedDefault::Uuid),
        ("cuid()", GeneratedDefault::Cuid),
        ("UUID()", GeneratedDefault::Uuid),
    ] {
        assert_eq!(
            round_trip(ColumnDefault::Function(call.to_owned())),
            Some(ColumnDefault::Generated(generated)),
            "{call}"
        );
    }
}

#[test]
fn other_functions_and_literals_are_kept() {
    for stored in [
        ColumnDefault::Function("gen_random_uuid()".to_owned()),
        ColumnDefault::Literal("'pending'".to_owned()),
        ColumnDefault::Generated(GeneratedDefault::Cuid),
    ] {
        assert_eq!(round_trip(stored.clone()), Some(stored));
    }
}

/// Before cratestack#1209 `@default(auth().id)` was stored, and emitted, as
/// the literal `auth().id`.
#[test]
fn old_auth_defaults_read_as_no_default() {
    assert_eq!(
        round_trip(ColumnDefault::Literal("auth().id".to_owned())),
        None
    );
}

#[test]
fn a_missing_default_key_reads_as_none() {
    let json = r#"{"name":"c","ty":{"Scalar":"String"},"arity":"Required","primary_key":false}"#;
    assert_eq!(
        serde_json::from_str::<Column>(json).expect("parse").default,
        None
    );
}

/// Before cratestack#1234 `@default("draft")` was stored, and emitted, as
/// `"draft"`: a column reference to Postgres. It reads back as the SQL
/// string the emitter now writes.
#[test]
fn old_double_quoted_strings_read_as_sql_strings() {
    assert_eq!(
        round_trip(ColumnDefault::Literal(r#""it's \"x\"""#.to_owned())),
        Some(ColumnDefault::Literal(r#"'it''s "x"'"#.to_owned()))
    );
}
