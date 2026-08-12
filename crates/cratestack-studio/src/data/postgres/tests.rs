use cratestack_core::Schema;

use crate::data::model_info::{PkCast, resolve_model, version_column};
use crate::data::{Row, SqlOp};

use super::preview;
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
        build_update_sql(&info, &columns, None),
        build_delete_sql(&info),
    ] {
        assert!(sql.contains(r#""subject_id" AS "subjectId""#), "{sql}");
    }
}

/// cratestack#507 "option 3": `build_update_sql`'s `version_column`
/// appends a raw `col = col + 1` fragment, not a bound placeholder, so
/// it must not shift the positional `$N` index the trailing PK bind
/// relies on.
#[test]
fn update_sql_bumps_version_column_without_disturbing_pk_placeholder() {
    let schema = parse(
        r#"
            model Message {
              id String @id
              version Int @version
              stateReason String
            }
        "#,
    );
    let (_, info) = resolve_model(&schema, "Message").unwrap();
    let columns = vec!["state_reason".to_owned()];
    let sql = build_update_sql(&info, &columns, Some("version"));
    assert!(
        sql.contains(r#""state_reason" = $1, "version" = "version" + 1"#),
        "{sql}"
    );
    assert!(sql.contains(r#""id" = $2"#), "{sql}");
}

/// A version-only bump (no other columns changed — e.g. the client's
/// PATCH payload contained only a `version` key, which
/// `collect_payload` strips) must still produce valid SQL: no leading
/// comma before the bump fragment.
#[test]
fn update_sql_version_bump_with_no_other_columns_has_no_leading_comma() {
    let schema = parse(
        r#"
            model Message {
              id String @id
              version Int @version
            }
        "#,
    );
    let (_, info) = resolve_model(&schema, "Message").unwrap();
    let sql = build_update_sql(&info, &[], Some("version"));
    assert!(
        sql.contains(r#"SET "version" = "version" + 1 WHERE"#),
        "{sql}"
    );
    assert!(sql.contains(r#""id" = $1"#), "{sql}");
}

/// cratestack#507 post-merge regression (found in review of PR #553):
/// `preview_sql` always runs with `payload = None`
/// (`api/preview.rs`), and `SqlOp::Create`'s no-payload branch seeds
/// `@version` to 0 *in addition to* the sample columns, which used to
/// include it too (`sample_column_names` had no filter) — producing
/// `INSERT INTO "widgets" ("id", "version", "name", "version") ...`,
/// which Postgres rejects ("column ... specified more than once").
#[test]
fn create_preview_with_no_payload_names_version_column_once() {
    let schema = parse(
        r#"
            model Widget {
              id String @id
              version Int @version
              name String
            }
        "#,
    );
    let (model, info) = resolve_model(&schema, "Widget").unwrap();
    let version_col = version_column(model);
    let rendered = preview::render(
        &schema,
        &info,
        "Widget",
        version_col.as_deref(),
        SqlOp::Create,
        None,
        None,
    );
    let column_list = rendered
        .sql
        .split("VALUES")
        .next()
        .expect("insert has a VALUES clause");
    assert_eq!(
        column_list.matches(r#""version""#).count(),
        1,
        "version column must be named exactly once in the INSERT column list: {}",
        rendered.sql
    );
}

/// Same defect, `SqlOp::Update` shape: the sample columns used to
/// include `version` and `build_update_sql` additionally appends the
/// `"version" = "version" + 1` bump, giving two SET assignments to the
/// same column.
#[test]
fn update_preview_with_no_payload_sets_version_column_once() {
    let schema = parse(
        r#"
            model Widget {
              id String @id
              version Int @version
              name String
            }
        "#,
    );
    let (model, info) = resolve_model(&schema, "Widget").unwrap();
    let version_col = version_column(model);
    let rendered = preview::render(
        &schema,
        &info,
        "Widget",
        version_col.as_deref(),
        SqlOp::Update,
        Some("w1"),
        None,
    );
    assert_eq!(
        rendered.sql.matches(r#""version" = "#).count(),
        1,
        "version column must be SET exactly once: {}",
        rendered.sql
    );
}

/// Guards against overcorrecting: `collect_payload` already strips
/// `@version` from a real payload, so a `Some(payload)` preview must
/// still seed it exactly once on `Create`.
#[test]
fn create_preview_with_payload_still_seeds_version_column() {
    let schema = parse(
        r#"
            model Widget {
              id String @id
              version Int @version
              name String
            }
        "#,
    );
    let (model, info) = resolve_model(&schema, "Widget").unwrap();
    let version_col = version_column(model);
    let payload: Row = serde_json::from_value(serde_json::json!({ "name": "gadget" })).unwrap();
    let rendered = preview::render(
        &schema,
        &info,
        "Widget",
        version_col.as_deref(),
        SqlOp::Create,
        None,
        Some(&payload),
    );
    let column_list = rendered
        .sql
        .split("VALUES")
        .next()
        .expect("insert has a VALUES clause");
    assert_eq!(
        column_list.matches(r#""version""#).count(),
        1,
        "{}",
        rendered.sql
    );
}

/// Same guard for `Update`: a real payload must still get the bump.
#[test]
fn update_preview_with_payload_still_bumps_version_column() {
    let schema = parse(
        r#"
            model Widget {
              id String @id
              version Int @version
              name String
            }
        "#,
    );
    let (model, info) = resolve_model(&schema, "Widget").unwrap();
    let version_col = version_column(model);
    let payload: Row = serde_json::from_value(serde_json::json!({ "name": "gadget" })).unwrap();
    let rendered = preview::render(
        &schema,
        &info,
        "Widget",
        version_col.as_deref(),
        SqlOp::Update,
        Some("w1"),
        Some(&payload),
    );
    assert_eq!(
        rendered.sql.matches(r#""version" = "#).count(),
        1,
        "{}",
        rendered.sql
    );
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
