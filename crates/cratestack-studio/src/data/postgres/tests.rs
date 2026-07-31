use cratestack_core::Schema;

use crate::data::model_info::{PkCast, resolve_model};

use super::sql::{
    build_delete_sql, build_get_sql, build_insert_sql, build_list_on_column_sql, build_list_sql,
    build_update_sql,
};

fn parse(schema_text: &str) -> Schema {
    cratestack_parser::parse_schema(schema_text).expect("schema parses")
}

#[test]
fn list_sql_uses_text_cursor_predicate_for_string_pk() {
    let schema = parse(
        r#"
            model Post {
              id String @id
              title String
            }
        "#,
    );
    let (_, info) = resolve_model(&schema, "Post").unwrap();
    let sql = build_list_sql(&info, 50);
    assert!(sql.contains(r#""id" > $1"#), "{sql}");
    assert!(!sql.contains("::bigint"), "{sql}");
    assert!(sql.contains("LIMIT 50"), "{sql}");
    assert!(sql.contains(r#"FROM "posts""#), "{sql}");
}

#[test]
fn list_sql_casts_to_bigint_for_int_pk() {
    let schema = parse(
        r#"
            model Customer {
              id Int @id
              email String
            }
        "#,
    );
    let (_, info) = resolve_model(&schema, "Customer").unwrap();
    let sql = build_list_sql(&info, 10);
    assert_eq!(info.pk_cast, PkCast::BigInt);
    assert!(sql.contains(r#""id" > $1::bigint"#), "{sql}");
    assert!(sql.contains("LIMIT 10"), "{sql}");
}

#[test]
fn get_sql_uses_bigint_cast_for_int_pk() {
    let schema = parse(
        r#"
            model Customer {
              id Int @id
              email String
            }
        "#,
    );
    let (_, info) = resolve_model(&schema, "Customer").unwrap();
    let sql = build_get_sql(&info);
    assert!(sql.contains(r#""id" = $1::bigint"#), "{sql}");
    assert!(sql.contains("LIMIT 1"), "{sql}");
}

/// Every projected column must be aliased back to its `.cstack` field
/// name, because [`crate::data::Row`] promises field-name keys and the
/// UI, cursor extraction, relation follow, and the audit log all look
/// rows up that way.
///
/// The fixture deliberately uses **multi-word** field names: with only
/// single-word fields the camelCase name and the snake_case column name
/// coincide, so the bug this guards is invisible.
#[test]
fn projection_aliases_columns_back_to_cstack_field_names() {
    let schema = parse(
        r#"
            model Device {
              id String @id
              subjectId String
              jwkThumbprint String
              status String
            }
        "#,
    );
    let (_, info) = resolve_model(&schema, "Device").unwrap();
    for sql in [build_list_sql(&info, 50), build_get_sql(&info)] {
        assert!(sql.contains(r#""subject_id" AS "subjectId""#), "{sql}");
        assert!(
            sql.contains(r#""jwk_thumbprint" AS "jwkThumbprint""#),
            "{sql}"
        );
        // Single-word fields still round-trip (alias == column).
        assert!(sql.contains(r#""status" AS "status""#), "{sql}");
    }
}

/// The write paths project through the same helper, so a row returned
/// from INSERT/UPDATE/DELETE … RETURNING is field-keyed too — otherwise
/// the drawer would repopulate its edit form from a row it can't read.
#[test]
fn write_returning_projections_are_field_named() {
    let schema = parse(
        r#"
            model Device {
              id String @id
              subjectId String
            }
        "#,
    );
    let (_, info) = resolve_model(&schema, "Device").unwrap();
    let columns = vec!["subject_id".to_owned()];
    for sql in [
        build_insert_sql(&info, &columns),
        build_update_sql(&info, &columns),
        build_delete_sql(&info),
    ] {
        assert!(sql.contains(r#""subject_id" AS "subjectId""#), "{sql}");
    }
}

#[test]
fn list_on_column_filters_and_pages_simultaneously() {
    let schema = parse(
        r#"
            model Post {
              id String @id
              authorId String
              title String
            }
        "#,
    );
    let (_, info) = resolve_model(&schema, "Post").unwrap();
    let sql = build_list_on_column_sql(&info, "author_id", PkCast::Text, 25);
    assert!(sql.contains(r#""author_id" = $1"#), "{sql}");
    assert!(sql.contains(r#""id" > $2"#), "{sql}");
    assert!(sql.contains("LIMIT 25"), "{sql}");
}
