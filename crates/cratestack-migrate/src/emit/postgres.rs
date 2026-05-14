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
    AddColumn, AddIndex, AlterColumnDefault, AlterColumnNullability, AlterColumnType,
    AlterEnumAddVariant, Column, ColumnArity, ColumnDefault, ColumnType, CreateEnum, CreateTable,
    Destructiveness, DropColumn, DropEnum, DropIndex, Op, RenameColumn, RenameTable,
};
use crate::naming;

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
        Op::RenameTable(rename) => emit_rename_table(sql, rename),
        Op::RenameColumn(rename) => emit_rename_column(sql, rename),
        Op::CreateEnum(create) => emit_create_enum(sql, create),
        Op::AlterEnumAddVariant(alter) => emit_alter_enum_add(sql, alter),
        Op::DropEnum(drop) => emit_drop_enum(sql, drop),
    }
}

fn emit_create_enum(sql: &mut String, create: &CreateEnum) {
    let type_name = quote_ident(&naming::column_name(&create.name));
    let variants: Vec<String> = create
        .variants
        .iter()
        .map(|v| format!("'{}'", v.replace('\'', "''")))
        .collect();
    writeln!(
        sql,
        "CREATE TYPE {type_name} AS ENUM ({});",
        variants.join(", ")
    )
    .unwrap();
}

fn emit_alter_enum_add(sql: &mut String, alter: &AlterEnumAddVariant) {
    let type_name = quote_ident(&naming::column_name(&alter.name));
    writeln!(
        sql,
        "ALTER TYPE {type_name} ADD VALUE '{}';",
        alter.value.replace('\'', "''")
    )
    .unwrap();
}

fn emit_drop_enum(sql: &mut String, drop: &DropEnum) {
    let type_name = quote_ident(&naming::column_name(&drop.name));
    writeln!(sql, "DROP TYPE {type_name};").unwrap();
}

fn emit_rename_table(sql: &mut String, rename: &RenameTable) {
    writeln!(
        sql,
        "ALTER TABLE {} RENAME TO {};",
        quote_ident(&rename.from),
        quote_ident(&rename.to)
    )
    .unwrap();
}

fn emit_rename_column(sql: &mut String, rename: &RenameColumn) {
    writeln!(
        sql,
        "ALTER TABLE {} RENAME COLUMN {} TO {};",
        quote_ident(&rename.table),
        quote_ident(&rename.from),
        quote_ident(&rename.to)
    )
    .unwrap();
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
        Op::RenameTable(rename) => {
            let reverse = RenameTable {
                from: rename.to.clone(),
                to: rename.from.clone(),
            };
            emit_rename_table(sql, &reverse);
        }
        Op::RenameColumn(rename) => {
            let reverse = RenameColumn {
                table: rename.table.clone(),
                from: rename.to.clone(),
                to: rename.from.clone(),
            };
            emit_rename_column(sql, &reverse);
        }
        Op::CreateEnum(create) => {
            writeln!(
                sql,
                "DROP TYPE {};",
                quote_ident(&naming::column_name(&create.name))
            )
            .unwrap();
        }
        Op::AlterEnumAddVariant(_) => {
            // Postgres has no `DROP VALUE`. Reversal would require
            // the swap-dance, which the generator does not attempt
            // here. Comment for the reader.
            sql.push_str("-- AlterEnumAddVariant has no Postgres reversal; manual rebuild required.\n");
        }
        Op::DropTable(_) | Op::DropColumn(_) | Op::AlterColumnType(_) | Op::DropEnum(_) => {
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
        // Enum and composite type identifiers are snake-cased so the
        // SQL type name matches the convention used elsewhere in
        // the generator (tables, columns) and so that case-mismatched
        // references don't silently resolve to different identifiers
        // under Postgres's unquoted-lowercase rule.
        ColumnType::Enum(name) | ColumnType::UserDefined(name) => quote_ident(&naming::column_name(name)),
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
        Op::DropEnum(drop) => format!("DropEnum {}", drop.name),
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
    fn table_rename_emits_alter_table_rename_to() {
        let prev = schema(&with_models(
            r#"
model OldName {
  id Int @id
  label String
}
"#,
        ));
        let next = schema(&with_models(
            r#"
model NewName {
  id Int @id
  label String

  @@rename(from = "old_names")
}
"#,
        ));
        let migration = emit(&diff(&prev, &next));
        assert!(!migration.has_lossy, "up was: {}", migration.up);
        assert!(
            migration
                .up
                .contains("ALTER TABLE old_names RENAME TO new_names;"),
            "up was: {}",
            migration.up
        );
        // No drop/add — the table was renamed, not recreated.
        assert!(!migration.up.contains("DROP TABLE"));
        assert!(!migration.up.contains("CREATE TABLE"));
        assert!(
            migration
                .down
                .contains("ALTER TABLE new_names RENAME TO old_names;"),
            "down was: {}",
            migration.down
        );
    }

    #[test]
    fn column_rename_emits_alter_table_rename_column() {
        let prev = schema(&with_models(
            r#"
model Customer {
  id Int @id
  email String
}
"#,
        ));
        let next = schema(&with_models(
            r#"
model Customer {
  id Int @id
  emailAddress String @rename(from = "email")
}
"#,
        ));
        let migration = emit(&diff(&prev, &next));
        assert!(!migration.has_lossy);
        assert!(
            migration
                .up
                .contains("ALTER TABLE customers RENAME COLUMN email TO email_address;"),
            "up was: {}",
            migration.up
        );
        // No drop/add — the column was renamed, not recreated.
        assert!(!migration.up.contains("DROP COLUMN"));
        assert!(!migration.up.contains("ADD COLUMN"));
    }

    #[test]
    fn rename_without_matching_old_falls_back_to_add() {
        // A @rename(from = "doesnt_exist") on a brand-new column
        // can't match an existing column — the diff engine falls
        // back to AddColumn and ignores the rename marker.
        let prev = schema(&with_models(
            r#"
model Customer {
  id Int @id
}
"#,
        ));
        let next = schema(&with_models(
            r#"
model Customer {
  id Int @id
  emailAddress String? @rename(from = "nope")
}
"#,
        ));
        let migration = emit(&diff(&prev, &next));
        assert!(
            migration
                .up
                .contains("ALTER TABLE customers ADD COLUMN email_address TEXT;"),
            "up was: {}",
            migration.up
        );
        assert!(!migration.up.contains("RENAME COLUMN"));
    }

    #[test]
    fn enum_create_emits_create_type_and_uses_snake_case() {
        let prev = schema(&with_models(""));
        let next = schema(&with_models(
            r#"
enum OrderStatus {
  Pending
  Submitted
  Shipped
}

model Order {
  id Int @id
  status OrderStatus
}
"#,
        ));
        let migration = emit(&diff(&prev, &next));
        assert!(!migration.has_lossy);
        // Enum type DDL lands before the table that references it.
        let create_type_idx = migration
            .up
            .find("CREATE TYPE order_status AS ENUM")
            .expect("CREATE TYPE present");
        let create_table_idx = migration
            .up
            .find("CREATE TABLE orders")
            .expect("CREATE TABLE present");
        assert!(
            create_type_idx < create_table_idx,
            "CREATE TYPE must precede CREATE TABLE so the column can reference the enum"
        );
        // Column type references the snake-cased enum.
        assert!(
            migration.up.contains("status order_status NOT NULL"),
            "up was: {}",
            migration.up
        );
        // Variants are single-quoted.
        assert!(migration.up.contains("'Pending', 'Submitted', 'Shipped'"));
    }

    #[test]
    fn enum_add_variant_emits_alter_type_add_value() {
        let prev = schema(&with_models(
            r#"
enum OrderStatus {
  Pending
  Submitted
}

model Order {
  id Int @id
  status OrderStatus
}
"#,
        ));
        let next = schema(&with_models(
            r#"
enum OrderStatus {
  Pending
  Submitted
  Shipped
}

model Order {
  id Int @id
  status OrderStatus
}
"#,
        ));
        let migration = emit(&diff(&prev, &next));
        assert!(!migration.has_lossy);
        assert!(
            migration
                .up
                .contains("ALTER TYPE order_status ADD VALUE 'Shipped';"),
            "up was: {}",
            migration.up
        );
    }

    #[test]
    fn enum_drop_is_lossy_and_routes_to_error_stub() {
        let prev = schema(&with_models(
            r#"
enum LegacyStatus {
  Active
}
"#,
        ));
        let next = schema(&with_models(""));
        let migration = emit(&diff(&prev, &next));
        assert!(migration.has_lossy);
        assert!(
            migration.up.contains("DROP TYPE legacy_status;"),
            "up was: {}",
            migration.up
        );
        assert!(migration.down.contains("destructive migration"));
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
