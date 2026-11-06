//! Switching an enum column between names and ordinals
//! (`docs/design/enum-ordinals.md` §4). The stored values are rewritten
//! with `USING CASE`, since no cast turns `'Pending'` into `1`. Any default
//! is dropped first: Postgres cannot cast the old `'Pending'` default to
//! `SMALLINT` (or `1` back to text) during `ALTER ... TYPE`, and the diff's
//! own `AlterColumnDefault`, which runs after the type change, sets the new
//! one.

use std::fmt::Write as _;

use crate::ir::{AlterColumnType, EnumCast};

use super::idents::quote_ident;

pub(super) fn emit_enum_cast(sql: &mut String, alter: &AlterColumnType, cast: &EnumCast, to: &str) {
    let table = quote_ident(&alter.table);
    let column = quote_ident(&alter.column);
    writeln!(
        sql,
        "ALTER TABLE {table} ALTER COLUMN {column} DROP DEFAULT;"
    )
    .unwrap();
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
    // A value outside the mapping becomes NULL, and then fails `NOT NULL`
    // or the CHECK the diff re-adds: the migration stops instead of
    // inventing a value.
    writeln!(
        sql,
        "ALTER TABLE {table} ALTER COLUMN {column} TYPE {to} USING (CASE {column} {} END);",
        arms.join(" ")
    )
    .unwrap();
}
