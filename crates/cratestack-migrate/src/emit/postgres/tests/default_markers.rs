//! cratestack#1128: `@default` marker calls never reach a `DEFAULT` clause.
//! `autoincrement()` is not a function in Postgres (`ERROR: function
//! autoincrement() does not exist`), and `dbgenerated()` is a marker, not
//! SQL. `cratestack check` refuses the first, so these tests reach the
//! emitter the two ways that skip validation: a direct library call
//! (`parse_schema_unvalidated`) and a snapshot written before the refusal.

use super::super::emit;
use super::{schema, with_models};
use crate::diff::{diff, diff_projections};
use crate::ir::ColumnDefault;
use crate::snapshot::Snapshot;

/// The statements only: the emitter's `-- NOTE` comments name `DEFAULT`.
fn sql(migration: &str) -> String {
    let lines = migration
        .lines()
        .filter(|line| !line.trim_start().starts_with("--"));
    lines.collect::<Vec<_>>().join("\n")
}

const WIDGET_DBGENERATED: &str = "model Widget {\n  id Int @id @default(dbgenerated())\n}\n";

fn unvalidated(models: &str) -> cratestack_core::Schema {
    cratestack_parser::parse_schema_unvalidated(&with_models(models))
        .expect("unvalidated schema should parse")
}

/// A snapshot as `migrate diff` wrote it before cratestack#1128: the same
/// table, its default stored as the function `autoincrement()`.
fn stale_snapshot() -> Snapshot {
    let json = serde_json::to_string(&Snapshot::from_schema(&schema(&with_models(
        WIDGET_DBGENERATED,
    ))))
    .expect("snapshot serializes");
    assert!(json.contains("\"DbGenerated\""), "fixture shape: {json}");
    let stale = json.replace("\"DbGenerated\"", "{\"Function\":\"autoincrement()\"}");
    serde_json::from_str(&stale).expect("stale snapshot parses")
}

#[test]
fn autoincrement_default_emits_no_default_clause() {
    for models in [
        "model Widget {\n  id Int @id @default(autoincrement())\n}\n",
        "model Widget {\n  id Int @id @default(autoincrement ())\n}\n",
        "model Widget {\n  id Int @id @default(dbgenerated() || 'x')\n}\n",
        "model Widget {\n  id Int @id @default(AutoIncrement())\n}\n",
        "model Widget {\n  id Int @id @default(autoincrement)\n}\n",
    ] {
        let migration = emit(&diff(&schema(&with_models("")), &unvalidated(models)).unwrap());
        assert!(
            !sql(&migration.up).contains("DEFAULT"),
            "{models}: {}",
            migration.up
        );
        assert!(
            migration.up.contains("id BIGINT NOT NULL,"),
            "{}",
            migration.up
        );
        assert_eq!(
            migration.unverified_dbgenerated,
            vec![("widgets".to_owned(), "id".to_owned())]
        );
    }
}

#[test]
fn spaced_bare_dbgenerated_emits_no_default_clause() {
    let next = schema(&with_models(
        "model Gadget {\n  id Int @id @default(dbgenerated ())\n}\n",
    ));
    let migration = emit(&diff(&schema(&with_models("")), &next).unwrap());
    assert!(!sql(&migration.up).contains("DEFAULT"), "{}", migration.up);
    assert_eq!(
        migration.unverified_dbgenerated,
        vec![("gadgets".to_owned(), "id".to_owned())]
    );
}

#[test]
fn a_stale_snapshot_reads_the_marker_as_dbgenerated() {
    let snapshot = stale_snapshot();
    let column = &snapshot.projections.tables["widgets"].columns[0];
    assert_eq!(column.default, Some(ColumnDefault::DbGenerated));
}

/// Moving to `dbgenerated()` after upgrading is no change at all; without
/// the read normalisation it was a default change whose `down.sql` said
/// `SET DEFAULT autoincrement()`.
#[test]
fn a_stale_snapshot_diffs_clean_against_dbgenerated() {
    let next = Snapshot::from_schema(&schema(&with_models(WIDGET_DBGENERATED)));
    let ops = diff_projections(&stale_snapshot().projections, &next.projections).unwrap();
    let migration = emit(&ops);
    assert!(!migration.up.contains("autoincrement"), "{}", migration.up);
    assert!(
        !migration.down.contains("autoincrement"),
        "{}",
        migration.down
    );
    assert!(ops.is_empty(), "no change expected: {ops:?}");
}

/// Removing the default reverses into restoring the previous one, which
/// the stale snapshot would have made `SET DEFAULT autoincrement()`.
#[test]
fn a_stale_snapshot_never_restores_the_marker_in_down() {
    let next = Snapshot::from_schema(&schema(&with_models("model Widget {\n  id Int @id\n}\n")));
    let ops = diff_projections(&stale_snapshot().projections, &next.projections).unwrap();
    let migration = emit(&ops);
    assert!(!migration.up.contains("autoincrement"), "{}", migration.up);
    assert!(
        !migration.down.contains("autoincrement"),
        "{}",
        migration.down
    );
}
