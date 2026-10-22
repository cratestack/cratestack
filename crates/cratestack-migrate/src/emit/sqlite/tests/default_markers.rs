//! cratestack#1128 for the SQLite emitter: `@default` marker calls never
//! reach a `DEFAULT` clause (SQLite: `near "autoincrement": syntax error`).
//! See the Postgres twin for why these parse unvalidated.

use super::super::emit;
use super::{schema, with_models};
use crate::diff::diff;

/// The statements only: the emitter's `-- NOTE` comments name `DEFAULT`.
fn sql(migration: &str) -> String {
    let lines = migration
        .lines()
        .filter(|line| !line.trim_start().starts_with("--"));
    lines.collect::<Vec<_>>().join("\n")
}

#[test]
fn marker_defaults_emit_no_default_clause() {
    for (models, table) in [
        (
            "model Widget {\n  id Int @id @default(autoincrement())\n}\n",
            "widgets",
        ),
        (
            "model Widget {\n  id Int @id @default(autoincrement ())\n}\n",
            "widgets",
        ),
        (
            "model Widget {\n  id Int @id @default(dbgenerated() || 'x')\n}\n",
            "widgets",
        ),
        (
            "model Gadget {\n  id Int @id @default(dbgenerated ())\n}\n",
            "gadgets",
        ),
        (
            "model Widget {\n  id Int @id @default(AutoIncrement())\n}\n",
            "widgets",
        ),
        (
            "model Widget {\n  id Int @id @default(autoincrement)\n}\n",
            "widgets",
        ),
    ] {
        let next = cratestack_parser::parse_schema_unvalidated(&with_models(models))
            .expect("unvalidated schema should parse");
        let migration = emit(&diff(&schema(&with_models("")), &next).unwrap());
        assert!(
            !sql(&migration.up).contains("DEFAULT"),
            "{models}: {}",
            migration.up
        );
        assert_eq!(
            migration.unverified_dbgenerated,
            vec![(table.to_owned(), "id".to_owned())]
        );
    }
}
