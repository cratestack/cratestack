//! Postgres SQL emitter for the migration IR.
//!
//! Maps `.cstack` scalars to Postgres types (`String` → `TEXT`,
//! `Int` → `BIGINT`, `Uuid` → `UUID`, …), renders `CREATE TABLE` /
//! `ALTER TABLE` / `CREATE INDEX` / `DROP …` statements, and produces
//! a reversal `down.sql` when no op in the migration is lossy.
//!
//! This entry file owns the [`emit`] orchestration and the per-op
//! dispatch in [`emit_up_op`]; reverse-direction emission lives in
//! [`down`], and each operation group (tables, columns, indexes,
//! checks, enums) lives in a sibling submodule.

mod checks;
mod columns;
mod down;
mod enums;
mod idents;
mod indexes;
mod tables;
mod views;

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
use enums::{emit_alter_enum_add, emit_create_enum, emit_drop_enum};
use idents::quote_ident;
use indexes::{emit_add_index, emit_drop_index};
use tables::{emit_create_table, emit_rename_table};
use views::{
    emit_create_materialized_view, emit_create_view, emit_drop_materialized_view, emit_drop_view,
    emit_replace_view,
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
        Op::RenameTable(rename) => emit_rename_table(sql, rename),
        Op::RenameColumn(rename) => emit_rename_column(sql, rename),
        Op::CreateEnum(create) => emit_create_enum(sql, create),
        Op::AlterEnumAddVariant(alter) => emit_alter_enum_add(sql, alter),
        Op::DropEnum(drop) => emit_drop_enum(sql, drop),
        Op::AddCheck(check) => emit_add_check(sql, check),
        Op::DropCheck(check) => emit_drop_check(sql, check),
        Op::CreateView(view) => emit_create_view(sql, view),
        Op::DropView(view) => emit_drop_view(sql, view),
        Op::ReplaceView(view) => emit_replace_view(sql, view),
        Op::CreateMaterializedView(view) => emit_create_materialized_view(sql, view),
        Op::DropMaterializedView(view) => emit_drop_materialized_view(sql, view),
    }
}
