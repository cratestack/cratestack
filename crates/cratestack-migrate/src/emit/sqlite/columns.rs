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

use super::defaults::render_default;
use super::idents::quote_ident;

pub(super) fn emit_add_column(sql: &mut String, add: &AddColumn) {
    let expression_default = add
        .column
        .default
        .as_ref()
        .filter(|default| !matches!(default, ColumnDefault::Literal(_)))
        .and_then(render_default);
    if expression_default.is_some() {
        writeln!(
            sql,
            "-- SQLite refuses to add a column whose default is an expression\n\
             -- (\"Cannot add a column with non-constant default\") once the table\n\
             -- has rows. If {} has rows, rebuild the table below, in this file.",
            add.table
        )
        .unwrap();
    }
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
    // An enum switching between names and ordinals is the one type change
    // that is not a storage no-op here: the stored values themselves must
    // be rewritten (`docs/design/enum-ordinals.md` §4).
    if let Some(cast) = &alter.enum_cast {
        let arms: Vec<String> = cast
            .pairs
            .iter()
            .map(|(name, ordinal)| {
                let name = format!("'{}'", name.replace('\'', "''"));
                if cast.to_ordinals {
                    format!("WHEN {name} THEN {ordinal}")
                } else {
                    format!("WHEN {ordinal} THEN {name}")
                }
            })
            .collect();
        let column = quote_ident(&alter.column);
        writeln!(
            sql,
            "UPDATE {} SET {column} = CASE {column} {} END;",
            quote_ident(&alter.table),
            arms.join(" ")
        )
        .unwrap();
        return;
    }
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
         hand-write the rebuild below, in this file.",
        alter.table, alter.column, alter.from, alter.to
    )
    .unwrap();
}

pub(super) fn emit_alter_column_nullability(sql: &mut String, alter: &AlterColumnNullability) {
    writeln!(
        sql,
        "-- SQLite has no ALTER COLUMN for nullability. Changing\n\
         -- {}.{} from {:?} to {:?} requires a table rebuild — \
         hand-write the rebuild below, in this file.",
        alter.table, alter.column, alter.from, alter.to
    )
    .unwrap();
}

pub(super) fn emit_alter_column_default(sql: &mut String, alter: &AlterColumnDefault) {
    writeln!(
        sql,
        "-- SQLite has no ALTER COLUMN for defaults. To change the\n\
         -- default on {}.{} to {:?}, rebuild the table below, in this file.",
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
    if let Some(rendered) = column.default.as_ref().and_then(render_default) {
        buf.push_str(" DEFAULT ");
        buf.push_str(&rendered);
    }
    buf
}
