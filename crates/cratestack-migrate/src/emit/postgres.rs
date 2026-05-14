//! Postgres SQL emitter for the migration IR.
//!
//! Maps `.cstack` scalars to Postgres types (`String` → `TEXT`,
//! `Int` → `BIGINT`, `Uuid` → `UUID`, …), renders `CREATE TABLE` /
//! `ALTER TABLE` / `CREATE INDEX` / `DROP …` statements, and produces
//! a reversal `down.sql` when no op in the migration is lossy.
//!
//! Identifier quoting is intentionally minimal: model-derived
//! identifiers (`customers`, `order_count`) are pure snake_case and
//! Postgres-safe without quotes. Identifiers that happen to collide
//! with reserved words (`order`, `user`) are quoted with double
//! quotes.

use std::fmt::Write as _;

use crate::emit::EmittedMigration;
use crate::ir::{
    AddColumn, AddIndex, AlterColumnDefault, AlterColumnNullability, AlterColumnType, Column,
    ColumnArity, ColumnDefault, ColumnType, CreateTable, Destructiveness, DropColumn, DropIndex,
    Op,
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
        sql.push_str("-- A required column was added without a default. The migration\n");
        sql.push_str("-- will fail on a non-empty table unless an `up.pre.sql` backfills\n");
        sql.push_str("-- the affected columns before this statement runs.\n\n");
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
        sql.push_str(
            "DO $$ BEGIN RAISE EXCEPTION \
             'destructive migration; reversal must be hand-written'; END $$;\n",
        );
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
        Op::AlterColumnType(alter) => emit_alter_column_type(sql, alter),
        Op::AlterColumnNullability(alter) => emit_alter_column_nullability(sql, alter),
        Op::AlterColumnDefault(alter) => emit_alter_column_default(sql, alter),
    }
}

fn emit_down_op(sql: &mut String, op: &Op) {
    match op {
        // Reversals — only reached when no op in the migration is lossy.
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
        Op::AlterColumnNullability(alter) => {
            // Reverse a nullability flip by setting the previous arity back.
            let reverse = AlterColumnNullability {
                table: alter.table.clone(),
                column: alter.column.clone(),
                from: alter.to,
                to: alter.from,
            };
            emit_alter_column_nullability(sql, &reverse);
        }
        Op::AlterColumnDefault(alter) => {
            let reverse = AlterColumnDefault {
                table: alter.table.clone(),
                column: alter.column.clone(),
                from: alter.to.clone(),
                to: alter.from.clone(),
            };
            emit_alter_column_default(sql, &reverse);
        }
        Op::DropTable(_) | Op::DropColumn(_) | Op::AlterColumnType(_) => {
            // Lossy — routed through the error stub above. AlterColumnType
            // is conservatively lossy because the diff engine has no
            // widening/narrowing view.
        }
        Op::DropIndex(_) => {
            // Dropping an index is recoverable in principle but we
            // don't know the index definition here — the down body
            // would need to recreate it from the old schema, which
            // requires snapshot lookup. Punt: drop is treated as
            // one-way at the migration boundary.
        }
    }
}

fn emit_alter_column_type(sql: &mut String, alter: &AlterColumnType) {
    let rendered = render_type(&alter.to, alter.to_arity);
    writeln!(
        sql,
        "ALTER TABLE {} ALTER COLUMN {} TYPE {} USING ({}::{});",
        quote_ident(&alter.table),
        quote_ident(&alter.column),
        rendered,
        quote_ident(&alter.column),
        rendered
    )
    .unwrap();
}

fn emit_alter_column_nullability(sql: &mut String, alter: &AlterColumnNullability) {
    match (alter.from, alter.to) {
        (ColumnArity::Required, ColumnArity::Optional) => writeln!(
            sql,
            "ALTER TABLE {} ALTER COLUMN {} DROP NOT NULL;",
            quote_ident(&alter.table),
            quote_ident(&alter.column)
        )
        .unwrap(),
        (ColumnArity::Optional, ColumnArity::Required) => writeln!(
            sql,
            "ALTER TABLE {} ALTER COLUMN {} SET NOT NULL;",
            quote_ident(&alter.table),
            quote_ident(&alter.column)
        )
        .unwrap(),
        // List ↔ scalar flips reshape data and ride along with
        // AlterColumnType — no standalone nullability statement.
        _ => {}
    }
}

fn emit_alter_column_default(sql: &mut String, alter: &AlterColumnDefault) {
    match &alter.to {
        Some(default) => {
            let rendered = match default {
                ColumnDefault::Literal(value) => value.as_str(),
                ColumnDefault::Function(call) => call.as_str(),
            };
            writeln!(
                sql,
                "ALTER TABLE {} ALTER COLUMN {} SET DEFAULT {};",
                quote_ident(&alter.table),
                quote_ident(&alter.column),
                rendered
            )
            .unwrap();
        }
        None => writeln!(
            sql,
            "ALTER TABLE {} ALTER COLUMN {} DROP DEFAULT;",
            quote_ident(&alter.table),
            quote_ident(&alter.column)
        )
        .unwrap(),
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
    buf.push(' ');
    buf.push_str(&render_type(&column.ty, column.arity));
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

fn render_type(ty: &ColumnType, arity: ColumnArity) -> String {
    let base = match ty {
        ColumnType::Scalar(name) => scalar_to_postgres(name).to_owned(),
        ColumnType::Enum(name) => quote_ident(name),
        ColumnType::UserDefined(name) => quote_ident(name),
    };
    match arity {
        ColumnArity::List => format!("{base}[]"),
        _ => base,
    }
}

fn scalar_to_postgres(name: &str) -> &'static str {
    match name {
        "String" | "Cuid" => "TEXT",
        "Int" => "BIGINT",
        "Float" => "DOUBLE PRECISION",
        "Decimal" => "NUMERIC",
        "Boolean" => "BOOLEAN",
        "DateTime" => "TIMESTAMPTZ",
        "Date" => "DATE",
        "Json" => "JSONB",
        "Bytes" => "BYTEA",
        "Uuid" => "UUID",
        // Unknown scalars are passed through unquoted — the developer
        // is responsible for ensuring the name resolves to a Postgres
        // type. New built-ins should be added above.
        _ => "TEXT",
    }
}

fn quote_ident(name: &str) -> String {
    if is_reserved(name) {
        format!("\"{name}\"")
    } else {
        name.to_owned()
    }
}

/// Postgres reserved words that show up in `.cstack` table/column
/// names often enough to be worth quoting. Not the full SQL reserved
/// list — that would require quoting nearly everything. The macro
/// codegen already quotes these in queries, so the migration table
/// matches.
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
        Op::AlterColumnType(alter) => format!(
            "AlterColumnType {}.{} ({:?} -> {:?})",
            alter.table, alter.column, alter.from, alter.to
        ),
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
  provider = "postgresql"
  url = env("DATABASE_URL")
}}
{models}
"#
        )
    }

    #[test]
    fn create_table_emits_postgres_ddl() {
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
        assert!(!migration.has_lossy);
        assert!(!migration.has_blocking);
        assert!(
            migration.up.contains("CREATE TABLE accounts"),
            "up was: {}",
            migration.up
        );
        assert!(migration.up.contains("id BIGINT NOT NULL"));
        assert!(migration.up.contains("balance BIGINT NOT NULL"));
        assert!(migration.up.contains("note TEXT"));
        assert!(!migration.up.contains("note TEXT NOT NULL"));
        assert!(migration.up.contains("PRIMARY KEY (id)"));
        assert!(migration.down.contains("DROP TABLE accounts;"));
    }

    #[test]
    fn add_column_emits_alter_table() {
        let prev = schema(&with_models(
            r#"
model Account {
  id Int @id
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
        assert!(migration.up.contains("ALTER TABLE accounts ADD COLUMN balance BIGINT"));
        assert!(
            migration
                .down
                .contains("ALTER TABLE accounts DROP COLUMN balance;")
        );
    }

    #[test]
    fn blocking_migration_carries_warning_comment() {
        let prev = schema(&with_models(
            r#"
model Account {
  id Int @id
}
"#,
        ));
        let next = schema(&with_models(
            r#"
model Account {
  id Int @id
  status String
}
"#,
        ));
        let migration = emit(&diff(&prev, &next));
        assert!(migration.has_blocking);
        assert!(migration.up.contains("WARNING"));
        assert!(migration.up.contains("ALTER TABLE accounts ADD COLUMN status TEXT NOT NULL"));
    }

    #[test]
    fn lossy_migration_emits_error_stub_for_down() {
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
        assert!(migration.up.contains("ALTER TABLE accounts DROP COLUMN legacy;"));
        assert!(migration.down.contains("destructive migration"));
        assert!(migration.down.contains("DropColumn accounts.legacy"));
        assert!(!migration.down.contains("ADD COLUMN"));
    }

    #[test]
    fn unique_field_creates_unique_index() {
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
        assert!(migration.down.contains("DROP INDEX users_email_key;"));
    }

    #[test]
    fn reserved_column_name_is_quoted() {
        let prev = schema(&with_models(""));
        let next = schema(&with_models(
            r#"
model Item {
  id Int @id
  order Int
}
"#,
        ));
        let migration = emit(&diff(&prev, &next));
        assert!(migration.up.contains("\"order\" BIGINT NOT NULL"));
    }

    #[test]
    fn defaults_are_rendered() {
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
        assert!(
            migration.up.contains("status TEXT NOT NULL DEFAULT 'pending'"),
            "up was: {}",
            migration.up
        );
    }

    #[test]
    fn list_column_renders_as_array() {
        let prev = schema(&with_models(""));
        let next = schema(&with_models(
            r#"
model Tag {
  id Int @id
  names String[]
}
"#,
        ));
        let migration = emit(&diff(&prev, &next));
        assert!(
            migration.up.contains("names TEXT[] NOT NULL"),
            "up was: {}",
            migration.up
        );
    }

    #[test]
    fn loosening_required_to_optional_is_safe() {
        let prev = schema(&with_models(
            r#"
model Account {
  id Int @id
  status String
}
"#,
        ));
        let next = schema(&with_models(
            r#"
model Account {
  id Int @id
  status String?
}
"#,
        ));
        let migration = emit(&diff(&prev, &next));
        assert!(!migration.has_lossy);
        assert!(!migration.has_blocking);
        assert!(
            migration
                .up
                .contains("ALTER TABLE accounts ALTER COLUMN status DROP NOT NULL;"),
            "up was: {}",
            migration.up
        );
        assert!(
            migration
                .down
                .contains("ALTER TABLE accounts ALTER COLUMN status SET NOT NULL;"),
            "down was: {}",
            migration.down
        );
    }

    #[test]
    fn tightening_optional_to_required_is_blocking() {
        let prev = schema(&with_models(
            r#"
model Account {
  id Int @id
  status String?
}
"#,
        ));
        let next = schema(&with_models(
            r#"
model Account {
  id Int @id
  status String
}
"#,
        ));
        let migration = emit(&diff(&prev, &next));
        assert!(migration.has_blocking);
        assert!(
            migration
                .up
                .contains("ALTER TABLE accounts ALTER COLUMN status SET NOT NULL;"),
        );
        assert!(migration.up.contains("WARNING"));
    }

    #[test]
    fn type_change_is_lossy_and_uses_using_cast() {
        let prev = schema(&with_models(
            r#"
model Account {
  id Int @id
  amount Int
}
"#,
        ));
        let next = schema(&with_models(
            r#"
model Account {
  id Int @id
  amount Decimal
}
"#,
        ));
        let migration = emit(&diff(&prev, &next));
        assert!(migration.has_lossy);
        assert!(
            migration
                .up
                .contains("ALTER TABLE accounts ALTER COLUMN amount TYPE NUMERIC USING (amount::NUMERIC);"),
            "up was: {}",
            migration.up
        );
        assert!(migration.down.contains("destructive migration"));
    }

    #[test]
    fn default_change_emits_set_and_drop_default() {
        let prev = schema(&with_models(
            r#"
model Order {
  id Int @id
  status String @default('pending')
}
"#,
        ));
        let next = schema(&with_models(
            r#"
model Order {
  id Int @id
  status String @default('submitted')
}
"#,
        ));
        let migration = emit(&diff(&prev, &next));
        assert!(!migration.has_lossy);
        assert!(
            migration
                .up
                .contains("ALTER TABLE orders ALTER COLUMN status SET DEFAULT 'submitted';"),
            "up was: {}",
            migration.up
        );
        assert!(
            migration
                .down
                .contains("ALTER TABLE orders ALTER COLUMN status SET DEFAULT 'pending';"),
        );
    }

    #[test]
    fn dropping_default_emits_drop_default() {
        let prev = schema(&with_models(
            r#"
model Order {
  id Int @id
  status String @default('pending')
}
"#,
        ));
        let next = schema(&with_models(
            r#"
model Order {
  id Int @id
  status String
}
"#,
        ));
        let migration = emit(&diff(&prev, &next));
        assert!(
            migration
                .up
                .contains("ALTER TABLE orders ALTER COLUMN status DROP DEFAULT;"),
            "up was: {}",
            migration.up
        );
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
        assert!(!migration.has_lossy);
        assert!(!migration.has_blocking);
    }
}
