//! Graph mechanics for `ownership.rs`'s `compute_type_ownership` — split
//! out to keep that file under this repo's ~200-LoC convention. Pure
//! functions over a `Schema` + the "eligible" (enum/`type`) name set;
//! no knowledge of `TypeOwner`/`TypeOwnership` lives here.

use std::collections::{BTreeMap, BTreeSet};

use cratestack_core::{Model, Procedure};

use crate::types::base_type_name;

#[derive(Debug, Clone, PartialOrd, Ord, PartialEq, Eq)]
pub(super) enum Consumer {
    Model(String),
    /// One entry per procedure *name*, not one shared bucket — see
    /// `ownership.rs`'s module doc's "A `type` block can never reach a
    /// model" section for why this granularity matters even though every
    /// procedure still renders into the same `src/procedures.ts`.
    Procedure(String),
}

/// Eligible names directly referenced by `ty`'s own fields — one entry
/// per declared `type` block, used to seed `compute_type_ownership`'s
/// adjacency map.
pub(super) fn type_decl_adjacency<'a>(
    fields: impl Iterator<Item = &'a cratestack_core::Field>,
    eligible: &BTreeSet<&str>,
) -> BTreeSet<&'a str> {
    fields
        .map(|field| base_type_name(&field.ty))
        .filter(|name| eligible.contains(name))
        .collect()
}

pub(super) fn model_referenced_eligible_names<'a>(
    model: &'a Model,
    eligible: &BTreeSet<&str>,
) -> BTreeSet<&'a str> {
    type_decl_adjacency(
        crate::types::visible_model_fields(model).into_iter(),
        eligible,
    )
}

pub(super) fn procedure_referenced_eligible_names<'a>(
    procedure: &'a Procedure,
    eligible: &BTreeSet<&str>,
) -> BTreeSet<&'a str> {
    let mut names = BTreeSet::new();
    for arg in &procedure.args {
        let name = base_type_name(&arg.ty);
        if eligible.contains(name) {
            names.insert(name);
        }
    }
    let return_name = base_type_name(&procedure.return_type);
    if eligible.contains(return_name) {
        names.insert(return_name);
    }
    names
}

/// BFS from `roots` over `adjacency`, returning every eligible name
/// transitively reachable from them (including the roots themselves).
pub(super) fn reachable_set<'a>(
    roots: &BTreeSet<&'a str>,
    adjacency: &BTreeMap<&'a str, BTreeSet<&'a str>>,
) -> BTreeSet<&'a str> {
    let mut visited = BTreeSet::new();
    let mut stack: Vec<&str> = roots.iter().copied().collect();
    while let Some(name) = stack.pop() {
        if !visited.insert(name) {
            continue;
        }
        if let Some(neighbors) = adjacency.get(name) {
            stack.extend(neighbors.iter().copied());
        }
    }
    visited
}
