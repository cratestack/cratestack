//! Foreign-key DDL: ADD / DROP CONSTRAINT ... FOREIGN KEY.

use std::fmt::Write as _;

use crate::ir::{AddForeignKey, DropForeignKey};

use super::idents::quote_ident;

pub(super) fn emit_add_foreign_key(sql: &mut String, fk: &AddForeignKey) {
    writeln!(
        sql,
        "ALTER TABLE {} ADD CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({});",
        quote_ident(&fk.table),
        quote_ident(&fk.name),
        quote_ident(&fk.column),
        quote_ident(&fk.referenced_table),
        quote_ident(&fk.referenced_column),
    )
    .unwrap();
}

pub(super) fn emit_drop_foreign_key(sql: &mut String, drop: &DropForeignKey) {
    writeln!(
        sql,
        "ALTER TABLE {} DROP CONSTRAINT {};",
        quote_ident(&drop.table),
        quote_ident(&drop.name)
    )
    .unwrap();
}
