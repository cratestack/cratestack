//! Table-level DDL: CREATE TABLE and RENAME TABLE.

use std::fmt::Write as _;

use crate::ir::{CreateTable, RenameTable};

use super::columns::render_column;
use super::idents::quote_ident;

pub(super) fn emit_create_table(sql: &mut String, create: &CreateTable) {
    writeln!(sql, "CREATE TABLE {} (", quote_ident(&create.name)).unwrap();
    let mut lines: Vec<String> = create
        .columns
        .iter()
        .map(|column| format!("    {}", render_column(column)))
        .collect();
    let pk: Vec<&str> = create
        .columns
        .iter()
        .filter(|column| column.primary_key)
        .map(|column| column.name.as_str())
        .collect();
    if !pk.is_empty() {
        let quoted: Vec<String> = pk.iter().map(|name| quote_ident(name)).collect();
        lines.push(format!("    PRIMARY KEY ({})", quoted.join(", ")));
    }
    sql.push_str(&lines.join(",\n"));
    sql.push_str("\n);\n");
}

pub(super) fn emit_rename_table(sql: &mut String, rename: &RenameTable) {
    writeln!(
        sql,
        "ALTER TABLE {} RENAME TO {};",
        quote_ident(&rename.from),
        quote_ident(&rename.to)
    )
    .unwrap();
}
