//! Direct tests against the public `project` / `diff_projections` seam
//! (`crate::projection`, `crate::diff::diff_projections`), independent
//! of the `diff()` wrapper. These prove the seam Phase B (issue #204)
//! needs: a future live-database introspector can drive the
//! comparison engine purely from hand-built `Projections` values,
//! without ever holding a `cratestack_core::Schema`.

use crate::convert::TableProjection;
use crate::ir::{Column, ColumnArity, ColumnType, Op};
use crate::projection::{Projections, project};
use crate::{diff, diff_projections};

use super::{schema, with_models};

fn table(name: &str, columns: Vec<Column>) -> TableProjection {
    TableProjection {
        name: name.to_string(),
        rename_from: None,
        columns,
        column_renames: Vec::new(),
        indexes: Vec::new(),
        checks: Vec::new(),
        foreign_keys: Vec::new(),
    }
}

fn id_column() -> Column {
    Column {
        name: "id".to_string(),
        ty: ColumnType::Scalar("Int".to_string()),
        arity: ColumnArity::Required,
        default: None,
        primary_key: true,
    }
}

#[test]
fn diff_projections_creates_table_from_hand_built_ir_with_no_schema_involved() {
    let prev = Projections::default();
    let mut next = Projections::default();
    next.tables
        .insert("accounts".to_string(), table("accounts", vec![id_column()]));

    let ops = diff_projections(&prev, &next);
    assert_eq!(ops.len(), 1);
    match &ops[0] {
        Op::CreateTable(create) => {
            assert_eq!(create.name, "accounts");
            assert_eq!(create.columns.len(), 1);
            assert_eq!(create.columns[0].name, "id");
        }
        other => panic!("expected CreateTable, got {other:?}"),
    }
}

#[test]
fn diff_projections_drops_table_missing_from_next() {
    let mut prev = Projections::default();
    prev.tables
        .insert("accounts".to_string(), table("accounts", vec![id_column()]));
    let next = Projections::default();

    let ops = diff_projections(&prev, &next);
    assert_eq!(ops.len(), 1);
    assert!(matches!(&ops[0], Op::DropTable(drop) if drop.name == "accounts"));
}

#[test]
fn diff_projections_of_identical_projections_is_empty() {
    let mut projections = Projections::default();
    projections
        .tables
        .insert("accounts".to_string(), table("accounts", vec![id_column()]));

    assert!(diff_projections(&projections, &projections).is_empty());
}

/// `diff()` is a thin wrapper around `project()` + `diff_projections()`
/// (design doc §5.1) — calling them separately must produce exactly
/// the same op list as calling `diff()` directly.
#[test]
fn project_then_diff_projections_matches_diff() {
    let prev = schema(&with_models(""));
    let next = schema(&with_models(
        r#"
model Account {
  id Int @id
  balance Int
}
"#,
    ));

    let via_diff = diff(&prev, &next);
    let via_projections = diff_projections(&project(&prev), &project(&next));
    assert_eq!(via_diff, via_projections);
}
