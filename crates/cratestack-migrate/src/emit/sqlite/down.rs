//! Reverse-direction emission for the SQLite emitter.

use std::fmt::Write as _;

use crate::ir::{Op, RenameColumn, RenameTable};

use super::columns::emit_rename_column;
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
        Op::AlterColumnNullability(_) | Op::AlterColumnDefault(_) => {
            // Both already require a hand-written table rebuild on
            // SQLite. The reverse direction needs the same rebuild,
            // so we emit a comment pointer rather than fake SQL.
            sql.push_str(
                "-- SQLite alter reversal requires the same table rebuild as the forward op.\n",
            );
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
        Op::DropTable(_) | Op::DropColumn(_) | Op::DropIndex(_) | Op::AlterColumnType(_) => {
            // Routed through the error stub above when lossy.
        }
        Op::CreateEnum(_) | Op::AlterEnumAddVariant(_) | Op::DropEnum(_) => {
            // Enum ops have no SQLite footprint; nothing to reverse.
        }
        Op::AddCheck(_) | Op::DropCheck(_) => {
            sql.push_str(
                "-- SQLite CHECK constraint reversal requires the same table rebuild as the forward op.\n",
            );
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
        _ => format!("{op:?}"),
    }
}
