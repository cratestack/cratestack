use cratestack_core::Schema;

use crate::data::model_info::{PkCast, resolve_model};

use super::sql::{build_get_sql, build_list_on_column_sql, build_list_sql};

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
