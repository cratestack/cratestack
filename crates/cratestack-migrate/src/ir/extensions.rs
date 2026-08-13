//! Schema-level (not per-table) DDL for a `.cstack` `extension <name>
//! { }` declaration that has a real Postgres extension behind it (see
//! `docs/design/extensions.md` §6 — currently just `pgvector`'s
//! `vector` extension; `rate_limit` has no database counterpart).

use serde::{Deserialize, Serialize};

/// Ensures a Postgres extension is installed before any DDL that
/// depends on it (e.g. a `vector(n)` column) runs. Maps to `CREATE
/// EXTENSION IF NOT EXISTS <name>;` — idempotent and safe to re-run,
/// but the diff engine only emits it once, the first time a schema
/// transitions from not declaring the matching `.cstack` extension to
/// declaring it (see `crate::diff::extensions`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnsureExtension {
    /// The Postgres extension name (e.g. `"vector"`), not the
    /// `.cstack` extension name — they happen to match for pgvector,
    /// but the IR keeps them conceptually separate.
    pub name: String,
}
