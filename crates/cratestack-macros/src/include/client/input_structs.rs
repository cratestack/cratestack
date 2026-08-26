//! `Create<M>Input`/`Update<M>Input` struct emission for
//! `include_client_schema!`, split out of `client.rs` for the 200-LoC
//! file convention.
//!
//! cratestack#743 (`docs/design/route-suppression.md`): this is a pure
//! client SDK crate — unlike `include_server_schema!`'s `pub mod
//! inputs` (whose `Create<M>Input`/`Update<M>Input` the ORM/procedure
//! layer needs regardless of route suppression, per the design's
//! non-goal that a suppressed action's in-process usability is
//! unchanged), a suppressed `create`/`update` here has no other
//! consumer: `client/rest/model.rs` and `client/rpc/model.rs` already
//! omit the method that would have referenced the input type (same
//! `model_internal_actions` gate), so emitting the type anyway would be
//! exactly the "unreferenced Create<M>Input" the acceptance criteria
//! forbid.

use std::collections::BTreeSet;

use cratestack_core::Schema;

use crate::model::{generate_client_create_input_struct, generate_client_update_input_struct};

/// `(create_input_structs, update_input_structs)`, each already filtered
/// against `cratestack_core::model_internal_actions` — the one shared
/// source of truth every codegen surface consults.
pub(super) fn client_input_structs(
    schema: &Schema,
    model_name_set: &BTreeSet<&str>,
    enum_name_set: &BTreeSet<&str>,
) -> (Vec<proc_macro2::TokenStream>, Vec<proc_macro2::TokenStream>) {
    let create_input_structs = schema
        .models
        .iter()
        .filter(|&model| !cratestack_core::model_internal_actions(model).contains("create"))
        .map(|model| generate_client_create_input_struct(model, model_name_set, enum_name_set))
        .collect();
    let update_input_structs = schema
        .models
        .iter()
        .filter(|&model| !cratestack_core::model_internal_actions(model).contains("update"))
        .map(|model| generate_client_update_input_struct(model, model_name_set, enum_name_set))
        .collect();
    (create_input_structs, update_input_structs)
}
