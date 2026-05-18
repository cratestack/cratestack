//! SQLite view DDL emitters (ADR-0003).
//!
//! - `CREATE VIEW <name> AS <sql>` / `DROP VIEW IF EXISTS <name>`
//! - `ReplaceView` expands to `DROP VIEW IF EXISTS; CREATE VIEW`
//!   since SQLite has no `CREATE OR REPLACE VIEW`.
//! - `CreateMaterializedView` / `DropMaterializedView` are routed to
//!   `unreachable!` — the macro's embedded composer hard-errors at
//!   expansion time on `@@materialized`, so the diff engine should
//!   never produce one of these ops on a SQLite emit path.

use std::fmt::Write;

use crate::ir::{CreateView, DropView, ReplaceView};

use super::idents::quote_ident;

pub(super) fn emit_create_view(sql: &mut String, view: &CreateView) {
    writeln!(
        sql,
        "CREATE VIEW {} AS {};",
        quote_ident(&view.name),
        view.sql.trim()
    )
    .unwrap();
}

pub(super) fn emit_drop_view(sql: &mut String, view: &DropView) {
    writeln!(
        sql,
        "DROP VIEW IF EXISTS {};",
        quote_ident(&view.name)
    )
    .unwrap();
}

pub(super) fn emit_replace_view(sql: &mut String, view: &ReplaceView) {
    writeln!(
        sql,
        "DROP VIEW IF EXISTS {};",
        quote_ident(&view.name)
    )
    .unwrap();
    writeln!(
        sql,
        "CREATE VIEW {} AS {};",
        quote_ident(&view.name),
        view.sql.trim()
    )
    .unwrap();
}
