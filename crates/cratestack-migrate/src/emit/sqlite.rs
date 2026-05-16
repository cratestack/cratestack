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
//! 2. **Enums are not emitted** (slice 10). Variant changes are a
//!    Rust-side concern only — the runtime stores enum values as
//!    text via BLOB affinity.
//!
//! SQLite supports `ALTER TABLE … DROP COLUMN` from version 3.35
//! (March 2021), well below any version cratestack-rusqlite cares
//! about, so drops are emitted directly without the table-rebuild
//! dance.

mod checks;
mod columns;
mod down;
mod idents;
mod indexes;
mod tables;

#[cfg(test)]
mod tests;

use std::fmt::Write as _;

use crate::emit::EmittedMigration;
use crate::ir::{Destructiveness, Op};

use checks::{emit_add_check, emit_drop_check};
use columns::{
    emit_add_column, emit_alter_column_default, emit_alter_column_nullability,
    emit_alter_column_type, emit_drop_column, emit_rename_column,
};
use down::{describe_lossy, emit_down_op};
use idents::quote_ident;
use indexes::{emit_add_index, emit_drop_index};
use tables::{emit_create_table, emit_rename_table};

pub fn emit(ops: &[Op]) -> EmittedMigration {
    let mut has_lossy = false;
    let mut has_blocking = false;
    for op in ops {
        // Enum ops have no SQLite footprint — skip them.
        if matches!(
            op,
            Op::CreateEnum(_) | Op::AlterEnumAddVariant(_) | Op::DropEnum(_)
        ) {
            continue;
        }
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
        sql.push_str("-- A required column was added without a default. SQLite will\n");
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
        sql.push_str(
            "SELECT RAISE(FAIL, 'destructive migration; reversal must be hand-written');\n",
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
        Op::CreateEnum(_) | Op::AlterEnumAddVariant(_) | Op::DropEnum(_) => {
            // SQLite has no native enum type — see the module docs.
        }
        Op::AddCheck(check) => emit_add_check(sql, check),
        Op::DropCheck(check) => emit_drop_check(sql, check),
    }
}
