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
//! checks) lives in a sibling submodule.
//!
//! Enum-typed columns are emitted as `TEXT` plus a `CHECK (col IN
//! (...))` membership constraint rather than a native `CREATE TYPE
//! ... AS ENUM`. The generated ORM row decoders read enum fields as
//! `String` (issue #228), so a native enum column fails to decode on
//! every read; TEXT is also the representation the SQLite backend
//! already uses, so both backends now agree.

mod checks;
mod columns;
mod down;
mod extensions;
mod foreign_keys;
mod idents;
mod indexes;
mod tables;
mod up_pre;
mod views;

#[cfg(test)]
mod tests;

use std::fmt::Write as _;

use crate::emit::EmittedMigration;
use crate::ir::{
    BlockingReason, Destructiveness, Op, blocking_reasons, unverified_dbgenerated_columns,
};

use checks::{emit_add_check, emit_drop_check};
use columns::{
    emit_add_column, emit_alter_column_default, emit_alter_column_nullability,
    emit_alter_column_type, emit_drop_column, emit_rename_column,
};
use down::{describe_lossy, emit_down_op};
use extensions::emit_ensure_extension;
use foreign_keys::{emit_add_foreign_key, emit_drop_foreign_key};
use idents::quote_ident;
use indexes::{emit_add_index, emit_drop_index};
use tables::{emit_create_table, emit_rename_table};
use views::{
    emit_create_materialized_view, emit_create_view, emit_drop_materialized_view, emit_drop_view,
    emit_replace_view,
};

pub fn emit(ops: &[Op]) -> EmittedMigration {
    let mut has_lossy = false;
    for op in ops {
        match op.destructiveness() {
            Destructiveness::Safe | Destructiveness::Blocking => {}
            Destructiveness::Lossy => has_lossy = true,
        }
    }

    // The reason list *is* the blocking bit — deriving `has_blocking`
    // from it rather than from a second pass means the warning text and
    // the flag the CLI prints can never disagree about whether this
    // migration blocks.
    let blocking = blocking_reasons(ops);
    let unverified_dbgenerated = unverified_dbgenerated_columns(ops);

    EmittedMigration {
        up_pre: (!blocking.is_empty()).then(|| up_pre::scaffold(&blocking)),
        up: emit_up(ops, &blocking, &unverified_dbgenerated),
        down: emit_down(ops, has_lossy),
        has_lossy,
        has_blocking: !blocking.is_empty(),
        unverified_dbgenerated,
    }
}

fn emit_up(
    ops: &[Op],
    blocking: &[BlockingReason],
    unverified_dbgenerated: &[(String, String)],
) -> String {
    let mut sql = String::new();
    if !blocking.is_empty() {
        sql.push_str(&up_pre::up_warning(blocking));
    }
    if !unverified_dbgenerated.is_empty() {
        sql.push_str("-- NOTE: the following column(s) use `@default(dbgenerated())`, a\n");
        sql.push_str("-- marker meaning the value is expected to come from a real\n");
        sql.push_str("-- Postgres-level default set some other way (hand-authored SQL, a\n");
        sql.push_str("-- trigger, GENERATED ... AS IDENTITY, etc). cratestack does not\n");
        sql.push_str("-- emit a DEFAULT clause for it. If no such default exists,\n");
        sql.push_str("-- INSERTs that omit the column will fail with a NOT NULL violation:\n");
        for (table, column) in unverified_dbgenerated {
            writeln!(sql, "--   - {table}.{column}").ok();
        }
        sql.push('\n');
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
        Op::AddCheck(check) => emit_add_check(sql, check),
        Op::DropCheck(check) => emit_drop_check(sql, check),
        Op::AddForeignKey(fk) => emit_add_foreign_key(sql, fk),
        Op::DropForeignKey(fk) => emit_drop_foreign_key(sql, fk),
        Op::CreateView(view) => emit_create_view(sql, view),
        Op::DropView(view) => emit_drop_view(sql, view),
        Op::ReplaceView(view) => emit_replace_view(sql, view),
        Op::CreateMaterializedView(view) => emit_create_materialized_view(sql, view),
        Op::DropMaterializedView(view) => emit_drop_materialized_view(sql, view),
        Op::EnsureExtension(op) => emit_ensure_extension(sql, op),
    }
}
