//! Op payloads for `view` and `materialized view` DDL (ADR-0003).
//!
//! Each variant carries the **dialect-active** SQL body — the diff
//! engine inspects both `server_sql` and `embedded_sql` on the
//! [`crate::convert::ViewProjection`] and forwards the right one into
//! the op for the emitter that's running.

use serde::{Deserialize, Serialize};

/// `CREATE VIEW <name> AS <sql>`.
///
/// `source_tables` is the list of SQL identifier names (snake_case +
/// pluralized) of source models the view reads from. The diff engine
/// uses it as a topological hint so view creates land after their
/// source-table creates and view drops land before their source-table
/// drops in the emitted op vec.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateView {
    pub name: String,
    pub sql: String,
    pub source_tables: Vec<String>,
}

/// `DROP VIEW <name>` (or `DROP VIEW IF EXISTS <name>` per dialect).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DropView {
    pub name: String,
}

/// `CREATE OR REPLACE VIEW <name> AS <sql>` on Postgres; the SQLite
/// emitter expands this to `DROP VIEW IF EXISTS <name>; CREATE VIEW
/// <name> AS <sql>` since SQLite has no `OR REPLACE VIEW`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplaceView {
    pub name: String,
    pub sql: String,
}

/// `CREATE MATERIALIZED VIEW <name> AS <sql>` plus
/// `CREATE UNIQUE INDEX <name>_pkey ON <name> (<primary_key>)` so
/// `REFRESH MATERIALIZED VIEW CONCURRENTLY` works.
///
/// Server-only — the SQLite emitter rejects this variant with
/// `unreachable!` since the macro's embedded composer already hard-
/// errors at expansion time on `@@materialized` views.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateMaterializedView {
    pub name: String,
    pub sql: String,
    pub primary_key: String,
    pub source_tables: Vec<String>,
}

/// `DROP MATERIALIZED VIEW <name>`. Server-only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DropMaterializedView {
    pub name: String,
}
