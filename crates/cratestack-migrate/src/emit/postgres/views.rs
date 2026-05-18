//! Postgres view DDL emitters (ADR-0003).
//!
//! - `CREATE VIEW <name> AS <sql>` / `DROP VIEW <name>`
//! - `CREATE OR REPLACE VIEW <name> AS <sql>` — Postgres native;
//!   atomic and avoids a transient drop/create window.
//! - `CREATE MATERIALIZED VIEW <name> AS <sql>` plus
//!   `CREATE UNIQUE INDEX <name>_pkey ON <name> (<pk>)` so
//!   `REFRESH MATERIALIZED VIEW CONCURRENTLY` works (the index is
//!   the precondition for concurrent refresh — see ADR §"Materialized
//!   views").

use std::fmt::Write;

use crate::ir::{
    CreateMaterializedView, CreateView, DropMaterializedView, DropView, ReplaceView,
};

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
    writeln!(sql, "DROP VIEW {};", quote_ident(&view.name)).unwrap();
}

pub(super) fn emit_replace_view(sql: &mut String, view: &ReplaceView) {
    writeln!(
        sql,
        "CREATE OR REPLACE VIEW {} AS {};",
        quote_ident(&view.name),
        view.sql.trim()
    )
    .unwrap();
}

pub(super) fn emit_create_materialized_view(sql: &mut String, view: &CreateMaterializedView) {
    writeln!(
        sql,
        "CREATE MATERIALIZED VIEW {} AS {};",
        quote_ident(&view.name),
        view.sql.trim()
    )
    .unwrap();
    // Unique index on the `@id` column is the precondition for
    // `REFRESH MATERIALIZED VIEW CONCURRENTLY`. The validator
    // rejects `@@materialized` + `@@no_unique` so `primary_key` is
    // always non-empty here.
    writeln!(
        sql,
        "CREATE UNIQUE INDEX {}_pkey ON {} ({});",
        view.name,
        quote_ident(&view.name),
        quote_ident(&view.primary_key)
    )
    .unwrap();
}

pub(super) fn emit_drop_materialized_view(sql: &mut String, view: &DropMaterializedView) {
    writeln!(
        sql,
        "DROP MATERIALIZED VIEW {};",
        quote_ident(&view.name)
    )
    .unwrap();
}
