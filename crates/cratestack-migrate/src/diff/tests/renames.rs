//! `@@rename(from = "...")` and `@rename(from = "...")` end to end through
//! `convert/renames.rs`: the form the parser accepts renames the table or
//! column instead of dropping it (GHSA-69g4-xvcm-vm2j; the parser refuses
//! every other form).

use super::super::diff;
use super::{schema, with_models};
use crate::ir::Op;

#[test]
fn an_accepted_model_rename_renames_the_table() {
    let prev = schema(&with_models(
        "\nmodel Document {\n  id Int @id\n  title String\n}\n",
    ));
    for marker in [
        "@@rename(from = \"documents\")",
        "@@rename(from=\"documents\")",
        "@@rename( from  =  \"documents\" )",
    ] {
        let next = schema(&with_models(&format!(
            "\nmodel Doc {{\n  id Int @id\n  title String\n  {marker}\n}}\n"
        )));
        let ops = diff(&prev, &next).expect("diff should succeed");
        assert!(
            matches!(ops.as_slice(), [Op::RenameTable(rename)]
                if rename.from == "documents" && rename.to == "docs"),
            "{marker}: expected one RenameTable documents -> docs, got {ops:?}"
        );
    }
}

/// Without the marker the same change drops the old table — the data loss
/// an unread marker used to cause silently.
#[test]
fn without_the_marker_the_old_table_is_dropped() {
    let prev = schema(&with_models(
        "\nmodel Document {\n  id Int @id\n  title String\n}\n",
    ));
    let next = schema(&with_models(
        "\nmodel Doc {\n  id Int @id\n  title String\n}\n",
    ));
    let ops = diff(&prev, &next).expect("diff should succeed");
    assert!(
        ops.iter()
            .any(|op| matches!(op, Op::DropTable(drop) if drop.name == "documents")),
        "{ops:?}"
    );
}

/// A field's `@rename(from = "...")` in every form the parser accepts
/// renames the column; before, only `@@rename` was checked, and a form the
/// migrator could not read dropped the column instead.
#[test]
fn an_accepted_field_rename_renames_the_column() {
    let prev = schema(&with_models(
        "\nmodel Doc {\n  id Int @id\n  name String\n}\n",
    ));
    for marker in [
        "@rename(from = \"name\")",
        "@rename(from=\"name\")",
        "@rename(  from  =  \"name\"  )",
    ] {
        let next = schema(&with_models(&format!(
            "\nmodel Doc {{\n  id Int @id\n  title String {marker}\n}}\n"
        )));
        let ops = diff(&prev, &next).expect("diff should succeed");
        assert!(
            matches!(ops.as_slice(), [Op::RenameColumn(rename)]
                if rename.table == "docs" && rename.from == "name" && rename.to == "title"),
            "{marker}: expected one RenameColumn name -> title, got {ops:?}"
        );
    }
}

/// Every form the migrator would read as no marker — a drop of `name` and
/// an add of `title` — is refused before it can reach a migration.
#[test]
fn a_field_rename_the_migrator_cannot_read_never_reaches_it() {
    let prev = schema(&with_models(
        "\nmodel Doc {\n  id Int @id\n  name String\n}\n",
    ));
    let next = schema(&with_models(
        "\nmodel Doc {\n  id Int @id\n  title String\n}\n",
    ));
    let ops = diff(&prev, &next).expect("diff should succeed");
    assert!(
        ops.iter()
            .any(|op| matches!(op, Op::DropColumn(drop) if drop.column == "name")),
        "without a marker the column is dropped: {ops:?}"
    );
    for marker in [
        "@rename(from: \"name\")",
        "@rename(\"name\")",
        "@rename(from = 'name')",
        "@rename",
        "@rename(from = \"name\"),",
        "@renam(from = \"name\")",
    ] {
        let source = with_models(&format!(
            "\nmodel Doc {{\n  id Int @id\n  title String {marker}\n}}\n"
        ));
        assert!(
            cratestack_parser::parse_schema(&source).is_err(),
            "{marker} must be refused"
        );
    }
}
