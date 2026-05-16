//! Index DDL: CREATE INDEX and DROP INDEX.

use std::fmt::Write as _;

use crate::ir::{AddIndex, DropIndex};

use super::idents::quote_ident;

pub(super) fn emit_add_index(sql: &mut String, index: &AddIndex) {
    let unique = if index.unique { "UNIQUE " } else { "" };
    let columns: Vec<String> = index.columns.iter().map(|c| quote_ident(c)).collect();
    writeln!(
        sql,
        "CREATE {unique}INDEX {} ON {} ({});",
        quote_ident(&index.name),
        quote_ident(&index.table),
        columns.join(", ")
    )
    .unwrap();
}

pub(super) fn emit_drop_index(sql: &mut String, drop: &DropIndex) {
    writeln!(sql, "DROP INDEX {};", quote_ident(&drop.name)).unwrap();
}
