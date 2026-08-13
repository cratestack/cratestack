//! Model-level `@@index([...], using: ..., opclass: "...")` → a general
//! (non-unique) `CREATE INDEX`, optionally naming a non-default Postgres
//! access method / operator class (issue #156 — pgvector phase 2:
//! ivfflat/hnsw ANN indexes, though the attribute itself is not
//! pgvector-specific; see `docs/design/extensions.md` §6/§8).
//!
//! Mirrors [`super::uniques::composite_unique_indexes`]'s shape closely —
//! same `AddIndex` op, same skip-rather-than-error discipline — the only
//! difference is `unique: false` and that `using`/`opclass` are threaded
//! through from the parsed attribute instead of always being `None`.

use std::collections::HashSet;

use cratestack_core::{Model, parse_index_attribute};

use crate::ir::{AddIndex, Column};
use crate::naming::{column_name, index_name};

/// One `AddIndex` per `@@index([...])` on the model, in declaration
/// order.
///
/// Malformed attributes and names that don't resolve to a projected
/// column are skipped rather than surfaced as errors: `cratestack check`
/// is the gate for those (see
/// `cratestack_parser::validate::index_attribute`), and this function
/// also runs over the *previous* schema read back from
/// `schema.snapshot.json`, which may have been written by an older
/// toolchain. Emitting a half-resolved index there would produce DDL
/// referencing a column that does not exist.
pub(super) fn model_index_indexes(model: &Model, table: &str, columns: &[Column]) -> Vec<AddIndex> {
    let projected: HashSet<&str> = columns.iter().map(|c| c.name.as_str()).collect();

    let mut indexes = Vec::new();
    for attribute in &model.attributes {
        if !attribute.raw.starts_with("@@index(") {
            continue;
        }
        let Ok(parsed) = parse_index_attribute(&attribute.raw) else {
            continue;
        };
        let index_columns: Vec<String> = parsed
            .fields
            .iter()
            .map(|field| column_name(field))
            .collect();
        if !index_columns
            .iter()
            .all(|name| projected.contains(name.as_str()))
        {
            continue;
        }
        let borrowed: Vec<&str> = index_columns.iter().map(String::as_str).collect();
        indexes.push(AddIndex {
            name: index_name(table, &borrowed, parsed.using.as_deref()),
            table: table.to_owned(),
            columns: index_columns,
            unique: false,
            using: parsed.using,
            opclass: parsed.opclass,
        });
    }
    indexes
}
