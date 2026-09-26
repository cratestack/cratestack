//! Runtime dotted-key resolution for REST `?orderBy=`/`?sort=` keys that
//! cross to-one relations (`"author.profile.nickname"`).
//!
//! Mirrors the type-space-to-value-space move `relation_path` already made
//! for the typed builder (cratestack#253) — one `OrderCatalog` per model,
//! carrying only that model's own scalar columns and its own to-one
//! relation edges, rather than a pre-enumerated list of every dotted path
//! through the graph. [`resolve_order_target`] walks a key hop by hop
//! against these catalogs at request time, so codegen for the REST
//! dispatch surface stays linear in `models × fields` however densely
//! models are to-one-connected (cratestack#256 — the same exponential
//! shape as #252, but in the REST string-key match arms rather than the
//! typed builder's path types).

use crate::relation_path::RelationHop;

/// One model's order-by surface: its own sortable scalar columns
/// (`(api_name, sql_column)`) and its own to-one relation edges. Exactly
/// one `OrderCatalog` is emitted per model, regardless of how many
/// distinct relation paths pass through it.
pub struct OrderCatalog {
    pub scalars: &'static [(&'static str, &'static str)],
    pub relations: &'static [OrderRelationEdge],
}

/// One to-one relation edge out of a model. `target` points at the
/// related model's own catalog so [`resolve_order_target`] can keep
/// walking further segments; to-many relations are never represented
/// here (mirroring the codegen's existing to-one-only walk), so a key
/// that names one simply fails to resolve.
pub struct OrderRelationEdge {
    pub api_name: &'static str,
    pub hop: RelationHop,
    pub target: &'static OrderCatalog,
}

/// A dotted sort key resolved down to the relation hops to traverse plus
/// the terminal scalar column, ready for [`crate::OrderClause::relation_path`]
/// (each hop carries the related model's read scope).
pub struct ResolvedOrderTarget {
    pub hops: Vec<RelationHop>,
    pub column: &'static str,
}

/// Walk `key` (dot-separated, e.g. `"author.profile.nickname"`) through
/// `catalog`, following to-one relation edges one segment at a time and
/// resolving the final segment against the current model's scalar
/// columns.
///
/// Returns `None` for an unknown field, a relation segment with no
/// matching edge (including any to-many hop, which is never present in
/// the catalog), or a key whose last segment names a relation instead of
/// a scalar — every one of which the caller reports as the same
/// "unsupported sort field" validation error as any other bad key.
pub fn resolve_order_target(
    catalog: &'static OrderCatalog,
    key: &str,
) -> Option<ResolvedOrderTarget> {
    let mut hops = Vec::new();
    let mut current = catalog;
    let mut segments = key.split('.').peekable();

    loop {
        let segment = segments.next()?;
        if segments.peek().is_none() {
            return current
                .scalars
                .iter()
                .find(|(name, _)| *name == segment)
                .map(|(_, column)| ResolvedOrderTarget { hops, column });
        }
        let edge = current
            .relations
            .iter()
            .find(|edge| edge.api_name == segment)?;
        hops.push(edge.hop);
        current = edge.target;
    }
}

#[cfg(test)]
#[path = "order_catalog_tests.rs"]
mod tests;
