use super::*;

fn parse(text: &str) -> cratestack_core::Schema {
    cratestack_parser::parse_schema(text).expect("schema parses")
}

#[test]
fn maps_sqlite_unique_to_validation_error() {
    let schema = parse("model Post {\n  id String @id\n  title String\n}\n");
    let model = schema.models.iter().find(|m| m.name == "Post").unwrap();
    let err = rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error {
            code: rusqlite::ErrorCode::ConstraintViolation,
            extended_code: 2067,
        },
        Some("UNIQUE constraint failed: posts.id".to_owned()),
    );
    let mapped = map_sqlite_error(Some(model), &err).expect("mapped");
    match mapped {
        DataError::Validation(errs) => {
            assert_eq!(errs.len(), 1);
            assert_eq!(errs[0].field, "id");
            assert_eq!(errs[0].code, ValidationCode::Unique);
        }
        other => panic!("expected validation, got {other:?}"),
    }
}

#[test]
fn maps_sqlite_not_null_to_required() {
    let schema = parse("model Post {\n  id String @id\n  title String\n}\n");
    let model = schema.models.iter().find(|m| m.name == "Post").unwrap();
    let err = rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error {
            code: rusqlite::ErrorCode::ConstraintViolation,
            extended_code: 1299,
        },
        Some("NOT NULL constraint failed: posts.title".to_owned()),
    );
    let mapped = map_sqlite_error(Some(model), &err).expect("mapped");
    match mapped {
        DataError::Validation(errs) => {
            assert_eq!(errs[0].field, "title");
            assert_eq!(errs[0].code, ValidationCode::Required);
        }
        other => panic!("expected validation, got {other:?}"),
    }
}

#[test]
fn maps_sqlite_foreign_key_to_foreign_key_code() {
    let schema = parse("model Post {\n  id String @id\n  authorId Int\n}\n");
    let model = schema.models.iter().find(|m| m.name == "Post").unwrap();
    let err = rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error {
            code: rusqlite::ErrorCode::ConstraintViolation,
            extended_code: 787,
        },
        Some("FOREIGN KEY constraint failed".to_owned()),
    );
    let mapped = map_sqlite_error(Some(model), &err).expect("mapped");
    match mapped {
        DataError::Validation(errs) => {
            assert_eq!(errs[0].code, ValidationCode::ForeignKey);
        }
        other => panic!("expected validation, got {other:?}"),
    }
}

#[test]
fn extract_quoted_pulls_column_from_pg_message() {
    let msg = r#"null value in column "title" violates not-null constraint"#;
    assert_eq!(extract_quoted(msg, "column"), Some("title".to_owned()));
}
