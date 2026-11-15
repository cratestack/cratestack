//! A snapshot written by `migrate baseline` records which model and field each
//! table and column stores only because the baseline stamps it; one that does
//! not (an older snapshot, a bare introspection) must still refuse a changed
//! `@@map` / `@map` rather than read it as drop + create (cratestack#1375).

use super::super::diff_projections;
use super::{schema, with_models};
use crate::ir::Op;
use crate::projection::{Projections, project};

fn source(table: &str, column: &str, extra_model: &str, extra_field: &str) -> String {
    with_models(&format!(
        "\nmodel Bank {{\n  id Int @id\n  ssn String @map(\"{column}\") {extra_field}\n  \
         @@map(\"{table}\")\n  {extra_model}\n}}\n"
    ))
}

/// What `introspect` returns for the adopted database: no model, no map.
fn introspected(source: &str) -> Projections {
    let mut projections = project(&schema(source));
    for table in projections.tables.values_mut() {
        table.model = None;
        table.field_columns.clear();
    }
    projections
}

fn next(source: &str) -> Projections {
    project(&schema(source))
}

const ADOPTED: (&str, &str) = ("legacy_banks", "ssn_enc");

#[test]
fn a_table_map_change_after_baseline_is_refused_not_a_drop_and_create() {
    let prev = introspected(&source(ADOPTED.0, ADOPTED.1, "", ""));
    let result = diff_projections(&prev, &next(&source("bank_v2", ADOPTED.1, "", "")));
    let message = result
        .expect_err("must refuse, not DropTable + CreateTable")
        .to_string();
    assert!(
        message.contains("legacy_banks") && message.contains("bank_v2"),
        "{message}"
    );
    assert!(
        message.contains("@@rename(from = \"legacy_banks\")"),
        "{message}"
    );
}

#[test]
fn a_column_map_change_after_baseline_is_refused_not_a_drop_and_add() {
    let prev = introspected(&source(ADOPTED.0, ADOPTED.1, "", ""));
    let result = diff_projections(&prev, &next(&source(ADOPTED.0, "ssn_cipher", "", "")));
    let message = result
        .expect_err("must refuse, not DropColumn + AddColumn")
        .to_string();
    assert!(
        message.contains("ssn_enc") && message.contains("ssn_cipher"),
        "{message}"
    );
    assert!(message.contains("@rename(from = \"ssn_enc\")"), "{message}");
}

#[test]
fn a_declared_rename_after_baseline_is_a_rename() {
    let prev = introspected(&source(ADOPTED.0, ADOPTED.1, "", ""));
    let table = diff_projections(
        &prev,
        &next(&source(
            "bank_v2",
            ADOPTED.1,
            "@@rename(from = \"legacy_banks\")",
            "",
        )),
    )
    .expect("declared");
    assert!(
        matches!(table.as_slice(), [Op::RenameTable(r)] if r.from == "legacy_banks" && r.to == "bank_v2"),
        "{table:?}"
    );
    let column = diff_projections(
        &prev,
        &next(&source(
            ADOPTED.0,
            "ssn_cipher",
            "",
            "@rename(from = \"ssn_enc\")",
        )),
    )
    .expect("declared");
    assert!(
        matches!(column.as_slice(), [Op::RenameColumn(r)] if r.from == "ssn_enc" && r.to == "ssn_cipher"),
        "{column:?}"
    );
}

#[test]
fn an_unchanged_mapped_schema_after_baseline_has_no_ops() {
    let prev = introspected(&source(ADOPTED.0, ADOPTED.1, "", ""));
    assert_eq!(
        diff_projections(&prev, &next(&source(ADOPTED.0, ADOPTED.1, "", ""))).unwrap(),
        Vec::new()
    );
}

#[test]
fn dropping_one_model_and_adding_an_unmapped_one_is_still_a_drop_and_create() {
    // The refusal is for a mapped model with no history, not every new table.
    let prev = introspected(&with_models("\nmodel Old {\n  id Int @id\n}\n"));
    let ops = diff_projections(
        &prev,
        &next(&with_models("\nmodel Fresh {\n  id Int @id\n}\n")),
    )
    .expect("an unmapped new model is just a new table");
    assert!(
        ops.iter().any(|op| matches!(op, Op::DropTable(_))),
        "{ops:?}"
    );
}

#[test]
fn a_stamped_baseline_refuses_with_the_recorded_names() {
    use crate::error::MigrateError;
    let authored = next(&source(ADOPTED.0, ADOPTED.1, "", ""));
    let mut prev = introspected(&source(ADOPTED.0, ADOPTED.1, "", ""));
    crate::stamp_storage_names(&mut prev, &authored);
    let table = &prev.tables[ADOPTED.0];
    assert_eq!(table.model.as_deref(), Some("Bank"));
    assert_eq!(table.field_columns["ssn"], "ssn_enc");

    let moved = diff_projections(&prev, &next(&source("bank_v2", ADOPTED.1, "", "")));
    assert!(
        matches!(&moved, Err(MigrateError::TableStorageNameChanged { from, to, .. })
            if from == "legacy_banks" && to == "bank_v2"),
        "{moved:?}"
    );
    let remapped = diff_projections(&prev, &next(&source(ADOPTED.0, "ssn_cipher", "", "")));
    assert!(
        matches!(&remapped, Err(MigrateError::ColumnStorageNameChanged { from, to, .. })
            if from == "ssn_enc" && to == "ssn_cipher"),
        "{remapped:?}"
    );
}

#[test]
fn stamping_leaves_a_table_the_schema_does_not_store_alone() {
    let authored = next(&source(ADOPTED.0, ADOPTED.1, "", ""));
    let mut prev = introspected(&source("other_table", ADOPTED.1, "", ""));
    crate::stamp_storage_names(&mut prev, &authored);
    assert!(prev.tables["other_table"].model.is_none());
    assert!(prev.tables["other_table"].field_columns.is_empty());
}
