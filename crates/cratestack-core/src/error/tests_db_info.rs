//! `DbErrorInfo` builder and the `db_*` accessors on `CratestackError`.

use super::*;

fn full() -> DbErrorInfo {
    DbErrorInfo::new("new row violates check constraint")
        .with_sqlstate("23514")
        .with_constraint("items_qty_nonneg")
        .with_table("items")
        .with_column("qty")
        .with_schema("public")
}

#[test]
fn accessors_read_every_field_of_typed_database_and_conflict_errors() {
    for err in [
        CratestackError::DatabaseTyped(full()),
        CratestackError::ConflictTyped(full()),
    ] {
        assert_eq!(err.db_sqlstate(), Some("23514"));
        assert_eq!(err.db_constraint(), Some("items_qty_nonneg"));
        assert_eq!(err.db_table(), Some("items"));
        assert_eq!(err.db_column(), Some("qty"));
        assert_eq!(err.db_schema(), Some("public"));
        assert_eq!(err.db_message(), Some("new row violates check constraint"));
    }
}

#[test]
fn accessors_are_none_for_untyped_and_non_database_errors() {
    for err in [
        CratestackError::Database("boom".to_owned()),
        CratestackError::Internal("boom".to_owned()),
        CratestackError::NotFound("x".to_owned()),
    ] {
        assert_eq!(err.db_table(), None);
        assert_eq!(err.db_column(), None);
        assert_eq!(err.db_schema(), None);
        assert_eq!(err.db_message(), None);
    }
}

#[test]
fn new_info_has_only_the_detail_and_empty_detail_is_no_message() {
    let info = DbErrorInfo::new("m");
    assert_eq!(info.detail, "m");
    assert_eq!((&info.sqlstate, &info.constraint), (&None, &None));
    assert_eq!(
        (info.table(), info.column(), info.schema()),
        (None, None, None)
    );
    assert_eq!(
        CratestackError::DatabaseTyped(DbErrorInfo::default()).db_message(),
        None,
    );
}

/// `Display` keeps printing the server detail; the accessors, not `Display`,
/// are the log-safe path.
#[test]
fn display_is_unchanged() {
    assert_eq!(
        CratestackError::DatabaseTyped(full()).to_string(),
        "database: new row violates check constraint",
    );
}

/// A value serialized before the table/column/schema fields existed still
/// deserializes.
#[test]
fn info_without_the_new_fields_deserializes() {
    let json = r#"{"detail":"d","sqlstate":"23505","constraint":"k"}"#;
    let info: DbErrorInfo = serde_json::from_str(json).expect("old shape");
    assert_eq!(info.table(), None);
    assert_eq!(info.constraint.as_deref(), Some("k"));
}
/// `table`, `column` and `schema` live behind one box to keep
/// `CratestackError` (the `Err` of nearly every fallible function) within
/// clippy's `result_large_err` threshold of 128 bytes. Growing `DbErrorInfo`
/// past it would fail `just lint` workspace-wide.
#[test]
fn error_stays_within_the_large_err_threshold() {
    assert!(std::mem::size_of::<CratestackError>() <= 128);
}
