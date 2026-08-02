//! Foreign-key comments.
//!
//! SQLite only accepts a `FOREIGN KEY` clause inline in `CREATE
//! TABLE`; there is no `ALTER TABLE ADD/DROP CONSTRAINT` for it at
//! all, so both directions require a full table rebuild regardless of
//! whether the table is brand new or pre-existing. The emitter writes
//! a marker comment so the developer notices and hand-writes the
//! rebuild in `up.pre.sql` — mirrors `checks::emit_add_check`.

use std::fmt::Write as _;

use crate::ir::{AddForeignKey, DropForeignKey};

pub(super) fn emit_add_foreign_key(sql: &mut String, fk: &AddForeignKey) {
    write!(
        sql,
        "-- SQLite: ADD CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({})",
        fk.name, fk.column, fk.referenced_table, fk.referenced_column,
    )
    .unwrap();
    if let Some(action) = fk.on_delete.sql_keyword() {
        write!(sql, " ON DELETE {action}").unwrap();
    }
    if let Some(action) = fk.on_update.sql_keyword() {
        write!(sql, " ON UPDATE {action}").unwrap();
    }
    sql.push_str(" — requires table rebuild on SQLite. Hand-write up.pre.sql.\n");
}

pub(super) fn emit_drop_foreign_key(sql: &mut String, drop: &DropForeignKey) {
    writeln!(
        sql,
        "-- SQLite: DROP CONSTRAINT {} — requires table rebuild on SQLite.",
        drop.name
    )
    .unwrap();
}
