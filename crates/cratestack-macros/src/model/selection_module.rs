//! Per-model `mod selection { ... }` emission: the projection /
//! include surface (`Selection`, `IncludeSelection`, `Projected`,
//! `ProjectedInclude`) used by `.select(...)` / `.include(...)`
//! delegate calls. Wired into [`super::field_module`] under
//! `#[cfg(not(target_arch = "wasm32"))]`.

mod projected;
mod selection_struct;

use std::collections::BTreeSet;

use cratestack_core::Model;
use quote::quote;

use crate::shared::scalar_model_fields;

use super::selection;

pub(super) fn generate_selection_module(
    model: &Model,
    model_names: &BTreeSet<&str>,
    _models: &[Model],
) -> Result<proc_macro2::TokenStream, String> {
    let model_name = &model.name;
    let scalar_fields = scalar_model_fields(model, model_names);
    let selection_field_methods = selection::build_selection_field_methods(&scalar_fields);
    let include_selection_field_methods = selection_field_methods.clone();
    let selected_scalar_accessors =
        selection::build_selected_scalar_accessors(&scalar_fields, model_name);
    let included_scalar_accessors = selected_scalar_accessors.clone();

    let relation_entries =
        selection::build_selection_relation_entries(model, model_names, model_name)?;

    let include_methods: Vec<_> = relation_entries
        .iter()
        .map(|entry| entry.include_methods.clone())
        .collect();
    let include_fields: Vec<_> = relation_entries
        .iter()
        .map(|entry| entry.include_field.clone())
        .collect();
    let include_query_steps: Vec<_> = relation_entries
        .iter()
        .map(|entry| entry.include_query_step.clone())
        .collect();
    let include_accessors: Vec<_> = relation_entries
        .into_iter()
        .map(|entry| entry.include_accessor)
        .collect();

    let selection_block = selection_struct::build_selection_block(
        model_name,
        &selection_field_methods,
        &include_selection_field_methods,
        &include_methods,
        &include_query_steps,
    );
    let projected_block = projected::build_projected_block(
        model_name,
        &selected_scalar_accessors,
        &included_scalar_accessors,
        &include_accessors,
    );

    Ok(quote! {
        pub mod selection {
            #[derive(Debug, Clone, Default)]
            pub struct Includes {
                #(#include_fields)*
            }

            #selection_block
            #projected_block
        }
    })
}
