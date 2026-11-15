//! `@@map` / `@map` across a diff (cratestack#1375): a changed storage name is
//! refused unless a rename marker says so, never read as drop + create.

use super::super::{diff, diff_projections};
use super::{schema, with_models};
use crate::error::MigrateError;
use crate::ir::Op;
use crate::projection::{Projections, project};

const PLAIN: &str = "\nmodel Bank {\n  id Int @id\n  code String\n}\n";

fn bank(model_attrs: &str, code_attrs: &str) -> String {
    with_models(&format!(
        "\nmodel Bank {{\n  id Int @id\n  code String {code_attrs}\n  {model_attrs}\n}}\n"
    ))
}

/// The projections as a snapshot written before `model` / `field_columns`.
fn old_format(source: &str) -> Projections {
    let mut projections = project(&schema(source));
    for table in projections.tables.values_mut() {
        table.model = None;
        table.field_columns.clear();
    }
    projections
}

fn table_error(ops: Result<Vec<Op>, MigrateError>) -> (String, String, String) {
    match ops {
        Err(MigrateError::TableStorageNameChanged { model, from, to }) => (model, from, to),
        other => panic!("expected TableStorageNameChanged, got {other:?}"),
    }
}

#[test]
fn a_mapped_table_is_projected_under_its_name_with_its_columns() {
    let next = project(&schema(&bank("@@map(\"map_bank\")", "@map(\"bank_code\")")));
    let table = &next.tables["map_bank"];
    assert_eq!(table.model.as_deref(), Some("Bank"));
    assert_eq!(table.field_columns["code"], "bank_code");
    assert!(table.columns.iter().any(|c| c.name == "bank_code"));
}

#[test]
fn adding_map_to_an_existing_model_is_refused_with_the_rename_to_write() {
    let prev = schema(&with_models(PLAIN));
    let next = schema(&bank("@@map(\"map_bank\")", ""));
    let error = diff(&prev, &next).expect_err("must refuse");
    assert_eq!(
        table_error(Err(error)),
        ("Bank".into(), "banks".into(), "map_bank".into())
    );
    // The message names the exact marker to add.
    let message = diff(&prev, &next).unwrap_err().to_string();
    assert!(message.contains("@@rename(from = \"banks\")"), "{message}");

    // The same against a snapshot written before the fields existed.
    let result = diff_projections(&old_format(&with_models(PLAIN)), &project(&next));
    assert_eq!(
        table_error(result),
        ("Bank".into(), "banks".into(), "map_bank".into())
    );
}

#[test]
fn map_with_a_rename_is_one_rename_table() {
    let prev = schema(&with_models(PLAIN));
    let next = schema(&bank(
        "@@map(\"map_bank\")\n  @@rename(from = \"banks\")",
        "",
    ));
    for ops in [
        diff(&prev, &next),
        diff_projections(&old_format(&with_models(PLAIN)), &project(&next)),
    ] {
        let ops = ops.expect("a declared rename is fine");
        assert!(
            matches!(ops.as_slice(), [Op::RenameTable(r)] if r.from == "banks" && r.to == "map_bank"),
            "{ops:?}"
        );
    }
}

#[test]
fn changing_or_removing_map_is_refused_and_a_rename_allows_it() {
    let mapped = schema(&bank("@@map(\"map_bank\")", ""));
    let moved = schema(&bank("@@map(\"bank_v2\")", ""));
    let (model, from, to) = table_error(diff(&mapped, &moved));
    assert_eq!(
        (model.as_str(), from.as_str(), to.as_str()),
        ("Bank", "map_bank", "bank_v2")
    );
    let removed = schema(&with_models(PLAIN));
    assert_eq!(table_error(diff(&mapped, &removed)).2, "banks");

    let renamed = schema(&bank(
        "@@map(\"bank_v2\")\n  @@rename(from = \"map_bank\")",
        "",
    ));
    let ops = diff(&mapped, &renamed).expect("declared");
    assert!(
        matches!(ops.as_slice(), [Op::RenameTable(r)] if r.to == "bank_v2"),
        "{ops:?}"
    );
}

#[test]
fn adding_changing_or_removing_a_column_map_is_refused_unless_renamed() {
    let plain = schema(&with_models(PLAIN));
    let mapped = schema(&bank("", "@map(\"bank_code\")"));
    let error = diff(&plain, &mapped).expect_err("adding @map to a stored field");
    assert!(
        matches!(&error, MigrateError::ColumnStorageNameChanged { table, field, from, to, .. }
            if table == "banks" && field == "code" && from == "code" && to == "bank_code"),
        "{error:?}"
    );
    assert!(
        error.to_string().contains("@rename(from = \"code\")"),
        "{error}"
    );
    // Old-format snapshot: the previous column is read as the derived name.
    let old = diff_projections(&old_format(&with_models(PLAIN)), &project(&mapped));
    assert!(
        matches!(old, Err(MigrateError::ColumnStorageNameChanged { .. })),
        "{old:?}"
    );
    // Removing the marker is the same change the other way.
    assert!(matches!(
        diff(&mapped, &plain),
        Err(MigrateError::ColumnStorageNameChanged { .. })
    ));

    let renamed = schema(&bank("", "@map(\"bank_code\") @rename(from = \"code\")"));
    let ops = diff(&plain, &renamed).expect("declared");
    assert!(
        matches!(ops.as_slice(), [Op::RenameColumn(r)] if r.from == "code" && r.to == "bank_code"),
        "{ops:?}"
    );
}

#[test]
fn a_renamed_model_keeping_its_map_and_a_new_mapped_field_are_not_changes() {
    let prev = schema(&bank("@@map(\"map_bank\")", "@map(\"bank_code\")"));
    let renamed_model = schema(&with_models(
        "\nmodel Institution {\n  id Int @id\n  code String @map(\"bank_code\")\n  @@map(\"map_bank\")\n}\n",
    ));
    assert_eq!(
        diff(&prev, &renamed_model).expect("same storage"),
        Vec::new()
    );
    assert_eq!(diff(&prev, &prev).expect("same schema"), Vec::new());

    let with_field = schema(&with_models(
        "\nmodel Bank {\n  id Int @id\n  code String @map(\"bank_code\")\n  tag String? @map(\"tag_col\")\n  @@map(\"map_bank\")\n}\n",
    ));
    let ops = diff(&prev, &with_field).expect("a new field is an add");
    assert!(
        matches!(ops.as_slice(), [Op::AddColumn(add)] if add.column.name == "tag_col"),
        "{ops:?}"
    );
}

#[test]
fn the_snapshot_round_trips_and_an_introspected_table_is_unchanged() {
    let projections = project(&schema(&bank("@@map(\"map_bank\")", "@map(\"bank_code\")")));
    let json = serde_json::to_string(&projections).unwrap();
    let back: Projections = serde_json::from_str(&json).unwrap();
    assert_eq!(back, projections);

    // No model, no field map: neither key is written, so an introspected
    // projection serializes (and checksums) exactly as it did before.
    let bare = old_format(&with_models(PLAIN));
    let json = serde_json::to_string(&bare).unwrap();
    assert!(
        !json.contains("\"model\"") && !json.contains("field_columns"),
        "{json}"
    );
    let read: Projections = serde_json::from_str(&json).unwrap();
    assert_eq!(read, bare);
}
