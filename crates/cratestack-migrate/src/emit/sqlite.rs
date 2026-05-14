//! SQLite SQL emitter for the migration IR.
//!
//! Two SQLite-specific design choices, both inherited from the
//! `cratestack-rusqlite` runtime:
//!
//! 1. **Every column is declared `BLOB`.** SQLite's type affinity
//!    silently coerces stored values (TEXT-numbers become REAL under
//!    NUMERIC affinity, INTEGERs become text under TEXT affinity).
//!    BLOB affinity is the only one that preserves the bound storage
//!    class — see `crates/cratestack-rusqlite/src/ddl.rs` for the full
//!    discussion. Migrations must match the runtime's expectation,
//!    so every `.cstack` scalar maps to `BLOB` here.
//!
//! 2. **Enums are not emitted** (slice 10). The Postgres emitter will
//!    produce `CREATE TYPE … AS ENUM (…)`; the SQLite emitter ignores
//!    those ops entirely. The Rust enum type still drives serde at
//!    the runtime layer.
//!
//! SQLite supports `ALTER TABLE … DROP COLUMN` from version 3.35
//! (March 2021), which is well below any version cratestack-rusqlite
//! cares about, so drops are emitted directly without the historical
//! table-rebuild dance.

use std::fmt::Write as _;

use crate::emit::EmittedMigration;
use crate::ir::{
    AddColumn, AddIndex, Column, ColumnArity, ColumnDefault, CreateTable, Destructiveness,
    DropColumn, DropIndex, DropTable, Op,
};

pub fn emit(ops: &[Op]) -> EmittedMigration {
    let mut has_lossy = false;
    let mut has_blocking = false;
    for op in ops {
        match op.destructiveness() {
            Destructiveness::Safe => {}
            Destructiveness::Lossy => has_lossy = true,
            Destructiveness::Blocking => has_blocking = true,
        }
    }

    EmittedMigration {
        up: emit_up(ops, has_blocking),
        down: emit_down(ops, has_lossy),
        has_lossy,
        has_blocking,
    }
}

fn emit_up(ops: &[Op], has_blocking: bool) -> String {
    let mut sql = String::new();
    if has_blocking {
        sql.push_str("-- WARNING: this migration contains blocking operations.\n");
        sql.push_str(
            "-- A required column was added without a default. SQLite will\n",
        );
        sql.push_str("-- reject the ALTER TABLE … ADD COLUMN if the table is non-empty\n");
        sql.push_str("-- — supply a default in the schema or backfill via up.pre.sql.\n\n");
    }
    for op in ops {
        emit_up_op(&mut sql, op);
        sql.push('\n');
    }
    sql
}

fn emit_down(ops: &[Op], has_lossy: bool) -> String {
    if has_lossy {
        let mut sql = String::new();
        sql.push_str("-- This migration contains destructive operations and cannot be\n");
        sql.push_str("-- auto-reversed. Affected ops:\n");
        for op in ops {
            if op.destructiveness() == Destructiveness::Lossy {
                writeln!(sql, "--   - {}", describe_lossy(op)).ok();
            }
        }
        sql.push_str("--\n");
        sql.push_str("-- Write a real reverse migration before running `down`, or accept\n");
        sql.push_str("-- that this migration is forward-only.\n");
        sql.push_str("SELECT RAISE(FAIL, 'destructive migration; reversal must be hand-written');\n");
        return sql;
    }

    let mut sql = String::new();
    for op in ops.iter().rev() {
        emit_down_op(&mut sql, op);
        sql.push('\n');
    }
    sql
}

fn emit_up_op(sql: &mut String, op: &Op) {
    match op {
        Op::CreateTable(create) => emit_create_table(sql, create),
        Op::DropTable(drop) => writeln!(sql, "DROP TABLE {};", quote_ident(&drop.name)).unwrap(),
        Op::AddColumn(add) => emit_add_column(sql, add),
        Op::DropColumn(drop) => emit_drop_column(sql, drop),
        Op::AddIndex(index) => emit_add_index(sql, index),
        Op::DropIndex(drop) => emit_drop_index(sql, drop),
    }
}

fn emit_down_op(sql: &mut String, op: &Op) {
    match op {
        Op::CreateTable(create) => {
            writeln!(sql, "DROP TABLE {};", quote_ident(&create.name)).unwrap()
        }
        Op::AddColumn(add) => writeln!(
            sql,
            "ALTER TABLE {} DROP COLUMN {};",
            quote_ident(&add.table),
            quote_ident(&add.column.name)
        )
        .unwrap(),
        Op::AddIndex(index) => writeln!(sql, "DROP INDEX {};", quote_ident(&index.name)).unwrap(),
        Op::DropTable(_) | Op::DropColumn(_) | Op::DropIndex(_) => {
            // Routed through the error stub above when lossy; drop
            // index is treated as one-way at the migration boundary
            // (slice 8+ will track index definitions in the snapshot).
        }
    }
}

fn emit_create_table(sql: &mut String, create: &CreateTable) {
    writeln!(sql, "CREATE TABLE {} (", quote_ident(&create.name)).unwrap();
    let mut lines: Vec<String> = create
        .columns
        .iter()
        .map(|column| format!("    {}", render_column(column)))
        .collect();
    let pk: Vec<&str> = create
        .columns
        .iter()
        .filter(|column| column.primary_key)
        .map(|column| column.name.as_str())
        .collect();
    if !pk.is_empty() {
        let quoted: Vec<String> = pk.iter().map(|name| quote_ident(name)).collect();
        lines.push(format!("    PRIMARY KEY ({})", quoted.join(", ")));
    }
    sql.push_str(&lines.join(",\n"));
    sql.push_str("\n);\n");
}

fn emit_add_column(sql: &mut String, add: &AddColumn) {
    writeln!(
        sql,
        "ALTER TABLE {} ADD COLUMN {};",
        quote_ident(&add.table),
        render_column(&add.column)
    )
    .unwrap();
}

fn emit_drop_column(sql: &mut String, drop: &DropColumn) {
    writeln!(
        sql,
        "ALTER TABLE {} DROP COLUMN {};",
        quote_ident(&drop.table),
        quote_ident(&drop.column)
    )
    .unwrap();
}

fn emit_add_index(sql: &mut String, index: &AddIndex) {
    let unique = if index.unique { "UNIQUE " } else { "" };
    let columns: Vec<String> = index.columns.iter().map(|c| quote_ident(c)).collect();
    writeln!(
        sql,
        "CREATE {unique}INDEX {} ON {} ({});",
        quote_ident(&index.name),
        quote_ident(&index.table),
        columns.join(", ")
    )
    .unwrap();
}

fn emit_drop_index(sql: &mut String, drop: &DropIndex) {
    writeln!(sql, "DROP INDEX {};", quote_ident(&drop.name)).unwrap();
}

fn render_column(column: &Column) -> String {
    let mut buf = quote_ident(&column.name);
    // Every column is BLOB on SQLite — see the module docs.
    buf.push_str(" BLOB");
    if matches!(column.arity, ColumnArity::Required | ColumnArity::List) {
        buf.push_str(" NOT NULL");
    }
    if let Some(default) = &column.default {
        buf.push_str(" DEFAULT ");
        match default {
            ColumnDefault::Literal(value) => buf.push_str(value),
            ColumnDefault::Function(call) => buf.push_str(call),
        }
    }
    buf
}

fn quote_ident(name: &str) -> String {
    if is_reserved(name) {
        format!("\"{name}\"")
    } else {
        name.to_owned()
    }
}

fn is_reserved(name: &str) -> bool {
    matches!(
        name,
        "order"
            | "user"
            | "group"
            | "select"
            | "from"
            | "where"
            | "table"
            | "column"
            | "default"
            | "type"
            | "primary"
            | "foreign"
            | "references"
            | "constraint"
            | "check"
            | "unique"
            | "index"
            | "view"
            | "schema"
    )
}

fn describe_lossy(op: &Op) -> String {
    match op {
        Op::DropTable(drop) => format!("DropTable {}", drop.name),
        Op::DropColumn(drop) => format!("DropColumn {}.{}", drop.table, drop.column),
        _ => format!("{op:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::diff;
    use cratestack_core::Schema;
    use cratestack_parser::parse_schema;

    fn schema(source: &str) -> Schema {
        parse_schema(source).expect("schema should parse")
    }

    fn with_models(models: &str) -> String {
        format!(
            r#"
datasource db {{
  provider = "sqlite"
  url = env("DATABASE_URL")
}}
{models}
"#
        )
    }

    #[test]
    fn create_table_emits_blob_columns() {
        let prev = schema(&with_models(""));
        let next = schema(&with_models(
            r#"
model Account {
  id Int @id
  balance Int
  note String?
}
"#,
        ));
        let migration = emit(&diff(&prev, &next));
        assert!(migration.up.contains("CREATE TABLE accounts"));
        // Every scalar maps to BLOB per the rusqlite affinity contract.
        assert!(migration.up.contains("id BLOB NOT NULL"));
        assert!(migration.up.contains("balance BLOB NOT NULL"));
        assert!(migration.up.contains("note BLOB"));
        assert!(!migration.up.contains("note BLOB NOT NULL"));
        assert!(migration.up.contains("PRIMARY KEY (id)"));
        assert!(!migration.up.contains("BIGINT"));
        assert!(!migration.up.contains("TEXT"));
    }

    #[test]
    fn add_and_drop_column_use_alter_table() {
        let prev = schema(&with_models(
            r#"
model Account {
  id Int @id
  legacy String?
}
"#,
        ));
        let next = schema(&with_models(
            r#"
model Account {
  id Int @id
  balance Int?
}
"#,
        ));
        let migration = emit(&diff(&prev, &next));
        assert!(migration.up.contains("ALTER TABLE accounts DROP COLUMN legacy;"));
        assert!(migration.up.contains("ALTER TABLE accounts ADD COLUMN balance BLOB"));
    }

    #[test]
    fn lossy_migration_uses_raise_fail_stub() {
        let prev = schema(&with_models(
            r#"
model Account {
  id Int @id
  legacy String?
}
"#,
        ));
        let next = schema(&with_models(
            r#"
model Account {
  id Int @id
}
"#,
        ));
        let migration = emit(&diff(&prev, &next));
        assert!(migration.has_lossy);
        assert!(migration.down.contains("RAISE(FAIL"));
        assert!(migration.down.contains("DropColumn accounts.legacy"));
    }

    #[test]
    fn unique_index_emits_create_unique_index() {
        let prev = schema(&with_models(""));
        let next = schema(&with_models(
            r#"
model User {
  id Int @id
  email String @unique
}
"#,
        ));
        let migration = emit(&diff(&prev, &next));
        assert!(
            migration
                .up
                .contains("CREATE UNIQUE INDEX users_email_key ON users (email);"),
            "up was: {}",
            migration.up
        );
    }

    #[test]
    fn defaults_pass_through_unchanged() {
        let prev = schema(&with_models(""));
        let next = schema(&with_models(
            r#"
model Order {
  id Int @id
  status String @default('pending')
}
"#,
        ));
        let migration = emit(&diff(&prev, &next));
        assert!(migration.up.contains("status BLOB NOT NULL DEFAULT 'pending'"));
    }

    #[test]
    fn empty_diff_produces_empty_migration() {
        let s = schema(&with_models(
            r#"
model Account {
  id Int @id
}
"#,
        ));
        let migration = emit(&diff(&s, &s));
        assert_eq!(migration.up.trim(), "");
        assert_eq!(migration.down.trim(), "");
    }
}
