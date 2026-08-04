//! Reverse-direction emission. Only reached when no op in the
//! migration is lossy; otherwise [`super::emit_down`] writes the
//! error-stub body instead.

use std::fmt::Write as _;

use crate::ir::{
    AddCheck, AddForeignKey, AlterColumnDefault, AlterColumnNullability, DropCheck, DropForeignKey,
    Op, RenameColumn, RenameTable,
};

use super::checks::{emit_add_check, emit_drop_check};
use super::columns::{
    emit_alter_column_default, emit_alter_column_nullability, emit_rename_column,
};
use super::foreign_keys::{emit_add_foreign_key, emit_drop_foreign_key};
use super::idents::quote_ident;
use super::tables::emit_rename_table;

pub(super) fn emit_down_op(sql: &mut String, op: &Op) {
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
        Op::AddCheck(check) => {
            let reverse = DropCheck {
                table: check.table.clone(),
                column: check.column.clone(),
                name: check.name.clone(),
                kind: check.kind.clone(),
            };
            emit_drop_check(sql, &reverse);
        }
        Op::DropCheck(check) => {
            // `DropCheck` carries the predicate it dropped, so the
            // reversal is the matching `ADD CONSTRAINT`.
            let reverse = AddCheck {
                table: check.table.clone(),
                column: check.column.clone(),
                name: check.name.clone(),
                kind: check.kind.clone(),
            };
            emit_add_check(sql, &reverse);
        }
        Op::AddForeignKey(fk) => {
            let reverse = DropForeignKey {
                name: fk.name.clone(),
                table: fk.table.clone(),
                column: fk.column.clone(),
                referenced_table: fk.referenced_table.clone(),
                referenced_column: fk.referenced_column.clone(),
                on_delete: fk.on_delete,
                on_update: fk.on_update,
            };
            emit_drop_foreign_key(sql, &reverse);
        }
        Op::DropForeignKey(fk) => {
            let reverse = AddForeignKey {
                name: fk.name.clone(),
                table: fk.table.clone(),
                column: fk.column.clone(),
                referenced_table: fk.referenced_table.clone(),
                referenced_column: fk.referenced_column.clone(),
                on_delete: fk.on_delete,
                on_update: fk.on_update,
            };
            emit_add_foreign_key(sql, &reverse);
        }
        Op::CreateView(view) => {
            writeln!(sql, "DROP VIEW {};", quote_ident(&view.name)).unwrap();
        }
        Op::CreateMaterializedView(view) => {
            writeln!(sql, "DROP MATERIALIZED VIEW {};", quote_ident(&view.name)).unwrap();
        }
        Op::ReplaceView(_) => {
            // We can't reverse `CREATE OR REPLACE VIEW` without
            // knowing the previous SQL body — the diff engine knows
            // it, but down-emission only sees the new op. Emit a
            // marker so the migration's down body is honest.
            sql.push_str(
                "-- ReplaceView has no auto-reversal; the previous SQL body is not in the IR.\n",
            );
        }
        Op::DropTable(_)
        | Op::DropColumn(_)
        | Op::AlterColumnType(_)
        | Op::DropView(_)
        | Op::DropMaterializedView(_) => {
            // Lossy — routed through the error stub above.
            // AlterColumnType is conservatively lossy because the
            // diff engine has no widening/narrowing view.
        }
        Op::DropIndex(_) => {
            // Dropping an index is recoverable in principle but we
            // don't know the index definition here — the down body
            // would need to recreate it from the old schema, which
            // requires snapshot lookup. Punt: drop is treated as
            // one-way at the migration boundary.
        }
        Op::EnsureExtension(_) => {
            // Not reversed: `DROP EXTENSION` risks breaking other
            // objects that came to depend on it after this migration
            // ran (other columns, indexes) — a no-op is the safe
            // default, mirroring `DropIndex`'s one-way stance above.
        }
    }
}

pub(super) fn describe_lossy(op: &Op) -> String {
    match op {
        Op::DropTable(drop) => format!("DropTable {}", drop.name),
        Op::DropColumn(drop) => format!("DropColumn {}.{}", drop.table, drop.column),
        Op::AlterColumnType(alter) => format!(
            "AlterColumnType {}.{} ({:?} -> {:?})",
            alter.table, alter.column, alter.from, alter.to
        ),
        Op::DropView(drop) => format!("DropView {}", drop.name),
        Op::DropMaterializedView(drop) => format!("DropMaterializedView {}", drop.name),
        _ => format!("{op:?}"),
    }
}
