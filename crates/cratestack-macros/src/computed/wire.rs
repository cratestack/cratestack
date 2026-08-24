//! The server composer's dedicated `wire` module: one wire-shape struct
//! per computed-bearing owner (`model` or `type`), reusing
//! [`crate::model::struct_only::generate_wire_model_struct`] /
//! [`crate::types::generate_wire_type_struct`] — themselves wire-scope
//! siblings of the existing `generate_client_model_struct`/
//! `generate_client_type_struct` used by `include_client_schema!`.
//!
//! Fixes the silent-drop bug documented in `docs/design/computed-
//! fields.md`'s "Exclusions" section: the server's embedded self/peer-
//! calling client (`crate::client::generate_client_module`, shared with
//! `include_client_schema!`) decoded every model/procedure response into
//! the server-side struct (`super::models::<Model>`,
//! `super::types::<Type>` — computed fields excluded by design), so a
//! server calling its own or a peer's API could never observe a resolved
//! computed value. `crate::client` now decodes computed-bearing owners
//! into `super::wire::<Owner>` instead — see that module's own doc.
//!
//! Emitted only when `bearing` is non-empty (`include::server`'s
//! orchestrator skips `pub mod wire { ... }` entirely otherwise) — a
//! schema with no `@computed` fields gets zero additional generated code,
//! matching every other computed-fields codegen path in this crate.

use std::collections::BTreeSet;

use cratestack_core::Schema;

use crate::model::struct_only::generate_wire_model_struct;
use crate::types::generate_wire_type_struct;

/// Wire-shape struct definitions for every computed-bearing owner in
/// `schema`, in schema order (models first, then types — matching
/// `crate::computed::compose::generate_compose_helpers`'s own order).
pub(crate) fn generate_wire_structs(
    schema: &Schema,
    model_names: &BTreeSet<&str>,
    enum_names: &BTreeSet<&str>,
    bearing: &BTreeSet<String>,
) -> Vec<proc_macro2::TokenStream> {
    let mut structs = Vec::new();

    for model in &schema.models {
        if !bearing.contains(&model.name) {
            continue;
        }
        structs.push(generate_wire_model_struct(
            model,
            model_names,
            enum_names,
            bearing,
        ));
    }

    for ty in &schema.types {
        if !bearing.contains(&ty.name) {
            continue;
        }
        structs.push(generate_wire_type_struct(ty, bearing));
    }

    structs
}

#[cfg(test)]
mod tests;
