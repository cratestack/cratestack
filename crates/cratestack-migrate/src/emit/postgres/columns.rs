//! Column-level DDL: ADD / DROP / RENAME / ALTER (type, nullability,
//! default), plus the `render_column` / `render_type` helpers that
//! [`super::tables::emit_create_table`] also leans on.

use std::fmt::Write as _;

use crate::ir::{
    AddColumn, AlterColumnDefault, AlterColumnNullability, AlterColumnType, Column, ColumnArity,
    ColumnDefault, ColumnType, DropColumn, RenameColumn,
};
use crate::naming;

use super::idents::quote_ident;

pub(super) fn emit_add_column(sql: &mut String, add: &AddColumn) {
    writeln!(
        sql,
        "ALTER TABLE {} ADD COLUMN {};",
        quote_ident(&add.table),
        render_column(&add.column)
    )
    .unwrap();
}

pub(super) fn emit_drop_column(sql: &mut String, drop: &DropColumn) {
    writeln!(
        sql,
        "ALTER TABLE {} DROP COLUMN {};",
        quote_ident(&drop.table),
        quote_ident(&drop.column)
    )
    .unwrap();
}

pub(super) fn emit_rename_column(sql: &mut String, rename: &RenameColumn) {
    writeln!(
        sql,
        "ALTER TABLE {} RENAME COLUMN {} TO {};",
        quote_ident(&rename.table),
        quote_ident(&rename.from),
        quote_ident(&rename.to)
    )
    .unwrap();
}

pub(super) fn emit_alter_column_type(sql: &mut String, alter: &AlterColumnType) {
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

pub(super) fn emit_alter_column_nullability(sql: &mut String, alter: &AlterColumnNullability) {
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

pub(super) fn emit_alter_column_default(sql: &mut String, alter: &AlterColumnDefault) {
    match &alter.to {
        Some(ColumnDefault::Literal(value)) => emit_set_default(sql, alter, value),
        Some(ColumnDefault::Function(call)) => emit_set_default(sql, alter, call),
        // `dbgenerated()` never has DDL to set — dropping any
        // previously-managed default hands the column back to
        // whatever external mechanism is expected to supply it.
        Some(ColumnDefault::DbGenerated) | None => writeln!(
            sql,
            "ALTER TABLE {} ALTER COLUMN {} DROP DEFAULT;",
            quote_ident(&alter.table),
            quote_ident(&alter.column)
        )
        .unwrap(),
    }
}

fn emit_set_default(sql: &mut String, alter: &AlterColumnDefault, rendered: &str) {
    writeln!(
        sql,
        "ALTER TABLE {} ALTER COLUMN {} SET DEFAULT {};",
        quote_ident(&alter.table),
        quote_ident(&alter.column),
        rendered
    )
    .unwrap();
}

pub(super) fn render_column(column: &Column) -> String {
    let mut buf = quote_ident(&column.name);
    buf.push(' ');
    buf.push_str(&render_type(&column.ty, column.arity));
    if matches!(column.arity, ColumnArity::Required | ColumnArity::List) {
        buf.push_str(" NOT NULL");
    }
    match &column.default {
        Some(ColumnDefault::Literal(value)) => {
            buf.push_str(" DEFAULT ");
            buf.push_str(value);
        }
        Some(ColumnDefault::Function(call)) => {
            buf.push_str(" DEFAULT ");
            buf.push_str(call);
        }
        // No DDL default for `dbgenerated()` — see `ColumnDefault::DbGenerated`.
        Some(ColumnDefault::DbGenerated) | None => {}
    }
    buf
}

fn render_type(ty: &ColumnType, arity: ColumnArity) -> String {
    let base = match ty {
        ColumnType::Scalar(name) => scalar_to_postgres(name).to_owned(),
        // Enum and composite type identifiers are snake-cased so the
        // SQL type name matches the convention used elsewhere in the
        // generator (tables, columns) and so that case-mismatched
        // references don't silently resolve to different identifiers
        // under Postgres's unquoted-lowercase rule.
        ColumnType::Enum(name) | ColumnType::UserDefined(name) => {
            quote_ident(&naming::column_name(name))
        }
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
