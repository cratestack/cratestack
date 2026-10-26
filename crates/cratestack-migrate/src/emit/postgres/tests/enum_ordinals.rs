//! Ordinal enums on Postgres (`docs/design/enum-ordinals.md` §2.2, §3.2,
//! §4): a `SMALLINT` column with a numeric CHECK and default, the
//! names ↔ ordinals conversion, and the history refusals. The parser still
//! refuses ordinal enums, so schemas are built unvalidated.

use super::super::emit;
use super::with_models;
use crate::diff::diff;
use crate::error::MigrateError;

fn unvalidated(source: &str) -> cratestack_core::Schema {
    cratestack_parser::parse_schema_unvalidated(source).expect("parses")
}

fn order(variants: &str) -> cratestack_core::Schema {
    unvalidated(&with_models(&format!(
        "enum Status {{\n{variants}\n}}\n\nmodel Order {{\n  id Int @id\n  status Status @default(Pending)\n}}\n"
    )))
}

const ORDINALS: &str = "  Pending = 1\n  Approved = 2\n  Refunded = 5";

fn up(prev: &cratestack_core::Schema, next: &cratestack_core::Schema) -> String {
    emit(&diff(prev, next).expect("diff succeeds")).up
}

#[test]
fn an_ordinal_enum_column_is_smallint_with_a_numeric_check_and_default() {
    let sql = up(&unvalidated(&with_models("")), &order(ORDINALS));
    assert!(sql.contains("status SMALLINT NOT NULL DEFAULT 1"), "{sql}");
    assert!(sql.contains("CHECK (status IN (1, 2, 5))"), "{sql}");
    assert!(!sql.contains("'Pending'"), "{sql}");
}

#[test]
fn a_string_enum_converts_to_ordinals_by_rewriting_its_values() {
    let names = order("  Pending\n  Approved\n  Refunded");
    let migration = emit(&diff(&names, &order(ORDINALS)).expect("diff succeeds"));
    let sql = &migration.up;
    let drop_default = sql.find("ALTER COLUMN status DROP DEFAULT").expect(sql);
    let retype = sql
        .find(
            "ALTER COLUMN status TYPE SMALLINT USING (CASE status \
             WHEN 'Pending' THEN 1 WHEN 'Approved' THEN 2 WHEN 'Refunded' THEN 5 END)",
        )
        .expect(sql);
    let set_default = sql.find("SET DEFAULT 1").expect(sql);
    let check = sql.find("CHECK (status IN (1, 2, 5))").expect(sql);
    assert!(
        drop_default < retype && retype < set_default && retype < check,
        "{sql}"
    );
    assert!(
        migration.has_lossy,
        "a table rewrite needs --allow-destructive"
    );
}

#[test]
fn ordinals_convert_back_to_names_the_same_way() {
    let names = order("  Pending\n  Approved\n  Refunded");
    let sql = up(&order(ORDINALS), &names);
    assert!(
        sql.contains("TYPE TEXT USING (CASE status WHEN 1 THEN 'Pending' WHEN 2 THEN 'Approved' WHEN 5 THEN 'Refunded' END)"),
        "{sql}"
    );
}

#[test]
fn appending_renaming_and_retiring_are_allowed() {
    let base = order(ORDINALS);
    for next in [
        "  Pending = 1\n  Approved = 2\n  Refunded = 5\n  Disputed = 6",
        "  Pending = 1\n  Accepted = 2\n  Refunded = 5",
        "  Pending = 1\n  Approved = 2\n  @@reserved(5)",
    ] {
        diff(&base, &order(next)).unwrap_or_else(|error| panic!("{next}: {error}"));
    }
    // A rename keeps the number, so the column and its CHECK are untouched.
    let renamed = up(
        &base,
        &order("  Pending = 1\n  Accepted = 2\n  Refunded = 5"),
    );
    assert!(!renamed.contains("ALTER COLUMN status TYPE"), "{renamed}");
    assert!(!renamed.contains("CHECK"), "{renamed}");
}

#[track_caller]
fn refused(prev: &str, next: &str, needle: &str) {
    match diff(&order(prev), &order(next)) {
        Err(MigrateError::EnumOrdinalHistory { enum_name, reason }) => {
            assert_eq!(enum_name, "Status");
            assert!(reason.contains(needle), "{reason}");
        }
        other => panic!("expected an ordinal-history refusal, got {other:?}"),
    }
}

#[test]
fn rewriting_the_history_is_refused() {
    refused(
        ORDINALS,
        "  Pending = 1\n  Approved = 3\n  Refunded = 5",
        "changed from 2 to 3",
    );
    refused(
        ORDINALS,
        "  Pending = 1\n  Approved = 2",
        "add `@@reserved(5)`",
    );
    let retired = "  Pending = 1\n  Approved = 2\n  @@reserved(5)";
    refused(
        retired,
        "  Pending = 1\n  Approved = 2",
        "dropped from `@@reserved`",
    );
    refused(
        retired,
        "  Pending = 1\n  Approved = 2\n  Late = 5\n  @@reserved(5)",
        "earlier migration retired",
    );
}

#[test]
fn a_projection_without_ordinal_enums_serializes_as_before() {
    let names = order("  Pending\n  Approved");
    let json = serde_json::to_string(&crate::project(&names)).unwrap();
    assert!(!json.contains("\"enums\""), "{json}");
    assert!(!json.contains("ordinals"), "{json}");
}
