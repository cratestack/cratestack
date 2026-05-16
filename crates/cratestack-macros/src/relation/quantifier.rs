//! Per-quantifier (`some`/`every`/`none`) module emission inside the
//! to-many container. Each variant gets its own `Path` struct + scalar
//! accessors + nested relation modules, so call sites can write
//! `posts.some().title().contains("foo")` etc.

use cratestack_core::{Field, Model};
use quote::quote;

use crate::shared::{ident, model_name_set, scalar_model_fields};

use super::filter_fns::generate_relation_filter_functions;
use super::path_method::generate_scalar_relation_path_method;
use super::recursive_entries::build_quantifier_relation_entry;
use super::scalar_builder::generate_scalar_relation_builder_module;
use super::types::{RelationFilterWrapperKind, RelationPathSegment, relation_link};

#[allow(clippy::too_many_arguments)]
pub(super) fn generate_relation_quantifier_module(
    parent_model: &Model,
    target_model: &Model,
    relation_field: &Field,
    parent_wrappers: &[RelationPathSegment],
    kind: RelationFilterWrapperKind,
    module_name: &str,
    visited: &[String],
    models: &[Model],
) -> Result<proc_macro2::TokenStream, String> {
    let module_ident = ident(module_name);
    let link = relation_link(parent_model, relation_field, models)?;
    let mut wrappers = parent_wrappers.to_vec();
    wrappers.push(RelationPathSegment { link, kind });
    let scalar_filter_fns = generate_relation_filter_functions(target_model, &wrappers, models)?;
    let model_names = model_name_set(models);
    let scalar_builder_modules = scalar_model_fields(target_model, &model_names)
        .into_iter()
        .map(|field| {
            generate_scalar_relation_builder_module(
                field,
                &wrappers,
                false,
                &wrappers[0].link,
                target_model,
                wrappers[0].link.related_table.as_str(),
                &[],
                models,
            )
        })
        .collect::<Result<Vec<_>, String>>()?;
    let scalar_path_methods = scalar_model_fields(target_model, &model_names)
        .into_iter()
        .map(generate_scalar_relation_path_method)
        .collect::<Vec<_>>();
    let relation_entries =
        collect_quantifier_relation_entries(target_model, &model_names, visited, &wrappers, models)?;
    let relation_path_methods = relation_entries
        .iter()
        .map(|(method, _)| method.clone())
        .collect::<Vec<_>>();
    let relation_modules = relation_entries
        .into_iter()
        .map(|(_, module)| module)
        .collect::<Vec<_>>();

    Ok(quote! {
        pub mod #module_ident {
            pub use super::*;

            pub struct Path;

            impl Path {
                #(#scalar_path_methods)*
                #(#relation_path_methods)*
            }

            #(#scalar_filter_fns)*
            #(#scalar_builder_modules)*
            #(#relation_modules)*
        }
    })
}

fn collect_quantifier_relation_entries(
    target_model: &Model,
    model_names: &std::collections::BTreeSet<&str>,
    visited: &[String],
    wrappers: &[RelationPathSegment],
    models: &[Model],
) -> Result<Vec<super::types::RelationModuleEntry>, String> {
    crate::shared::relation_model_fields(target_model, model_names)
        .into_iter()
        .map(|nested_relation| {
            build_quantifier_relation_entry(
                target_model,
                nested_relation,
                visited,
                wrappers,
                models,
            )
        })
        .collect::<Result<Vec<_>, String>>()
        .map(|entries| entries.into_iter().flatten().collect())
}
