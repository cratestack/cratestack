//! Reverse-direction emission. Only reached when no op in the
//! migration is lossy; otherwise [`super::emit_down`] writes the
//! error-stub body instead.

use std::fmt::Write as _;

use crate::ir::{
    AlterColumnDefault, AlterColumnNullability, DropCheck, Op, RenameColumn, RenameTable,
};

use super::checks::emit_drop_check;
use super::columns::{
    emit_alter_column_default, emit_alter_column_nullability, emit_rename_column,
};
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
        Op::CreateEnum(create) => {
            writeln!(
                sql,
                "DROP TYPE {};",
                quote_ident(&crate::naming::column_name(&create.name))
            )
            .unwrap();
        }
        Op::AlterEnumAddVariant(_) => {
            // Postgres has no `DROP VALUE`. Reversal would require
            // the swap-dance, which the generator does not attempt
            // here. Comment for the reader.
            sql.push_str(
                "-- AlterEnumAddVariant has no Postgres reversal; manual rebuild required.\n",
            );
        }
        Op::AddCheck(check) => {
            let reverse = DropCheck {
                table: check.table.clone(),
                column: check.column.clone(),
                name: check.name.clone(),
            };
            emit_drop_check(sql, &reverse);
        }
        Op::DropCheck(check) => {
            // We can't reverse DropCheck without knowing the
            // constraint's kind — the previous schema's projection
            // had it, but the down-emission step doesn't carry that
            // structure forward. Emit a marker.
            writeln!(
                sql,
                "-- DropCheck {} cannot be auto-reversed; the original CHECK predicate is no longer in the IR.",
                check.name
            )
            .unwrap();
        }
        Op::DropTable(_) | Op::DropColumn(_) | Op::AlterColumnType(_) | Op::DropEnum(_) => {
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
        Op::DropEnum(drop) => format!("DropEnum {}", drop.name),
        _ => format!("{op:?}"),
    }
}
