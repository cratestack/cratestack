//! Column-level DDL: ADD / DROP / RENAME / ALTER, plus `render_column`.
//!
//! Every column is declared `BLOB` — SQLite's BLOB affinity is the
//! only one that preserves the bound storage class. See the module
//! docs and `cratestack-rusqlite/src/ddl.rs` for the full discussion.

use std::fmt::Write as _;

use crate::ir::{
    AddColumn, AlterColumnDefault, AlterColumnNullability, AlterColumnType, Column, ColumnArity,
    ColumnDefault, DropColumn, RenameColumn,
};

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
    // BLOB affinity covers every `.cstack` scalar on SQLite. Pure
    // type changes (Int → String) are storage-no-ops because both
    // round-trip through BLOB. Only the list-vs-scalar shape change
    // would matter, and the IR routes that through `AlterColumnType`
    // alongside the type — we surface a comment so the developer
    // notices and can hand-write a table rebuild if needed.
    writeln!(
        sql,
        "-- SQLite: column {}.{} type changes from {:?} to {:?}. \
         All scalars share BLOB affinity, so this is a no-op at the\n\
         -- storage layer. If the shape changed (scalar ↔ list), \
         hand-write the rebuild in up.pre.sql.",
        alter.table, alter.column, alter.from, alter.to
    )
    .unwrap();
}

pub(super) fn emit_alter_column_nullability(sql: &mut String, alter: &AlterColumnNullability) {
    writeln!(
        sql,
        "-- SQLite has no ALTER COLUMN for nullability. Changing\n\
         -- {}.{} from {:?} to {:?} requires a table rebuild — \
         hand-write the migration in up.pre.sql / up.sql.",
        alter.table, alter.column, alter.from, alter.to
    )
    .unwrap();
}

pub(super) fn emit_alter_column_default(sql: &mut String, alter: &AlterColumnDefault) {
    writeln!(
        sql,
        "-- SQLite has no ALTER COLUMN for defaults. To change the\n\
         -- default on {}.{} to {:?}, rebuild the table in up.pre.sql.",
        alter.table, alter.column, alter.to
    )
    .unwrap();
}

pub(super) fn render_column(column: &Column) -> String {
    let mut buf = quote_ident(&column.name);
    // Every column is BLOB on SQLite — see the module docs.
    buf.push_str(" BLOB");
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
