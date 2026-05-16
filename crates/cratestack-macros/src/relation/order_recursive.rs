//! Recursive emitter for the per-relation `pub mod <field>` module.

use std::collections::BTreeSet;

use cratestack_core::Model;
use quote::quote;

use crate::shared::{ident, model_name_set, relation_model_fields, scalar_model_fields};

use super::filter_fns::generate_relation_filter_functions;
use super::path_method::generate_scalar_relation_path_method;
use super::recursive_entries::build_recursive_relation_entry;
use super::types::{RelationFilterWrapperKind, RelationLink, RelationPathSegment};

#[allow(clippy::too_many_arguments)]
pub(super) fn generate_relation_order_module_recursive(
    root_link: &RelationLink,
    root_model: &Model,
    current_model: &Model,
    root_table: &str,
    path_prefix: &[String],
    relation_field: &cratestack_core::Field,
    wrappers: &[RelationPathSegment],
    visited: &[String],
    models: &[Model],
    root_extra_path_methods: &[proc_macro2::TokenStream],
) -> Result<proc_macro2::TokenStream, String> {
    let module_ident = ident(&relation_field.name);
    let model_names = model_name_set(models);
    let allow_ordering = wrappers_allow_ordering(wrappers);
    let scalar_fns = generate_relation_scalar_order_functions(
        current_model,
        &model_names,
        root_link,
        root_model,
        root_table,
        path_prefix,
        models,
        allow_ordering,
    )?;
    let scalar_filter_fns = generate_relation_filter_functions(current_model, wrappers, models)?;
    let scalar_builder_modules = generate_relation_scalar_builder_modules(
        current_model,
        &model_names,
        wrappers,
        allow_ordering,
        root_link,
        root_model,
        root_table,
        path_prefix,
        models,
    )?;
    let scalar_path_methods = scalar_model_fields(current_model, &model_names)
        .into_iter()
        .map(generate_scalar_relation_path_method)
        .collect::<Vec<_>>();
    let relation_entries = relation_model_fields(current_model, &model_names)
        .into_iter()
        .map(|nested_relation| {
            build_recursive_relation_entry(
                current_model,
                nested_relation,
                visited,
                wrappers,
                path_prefix,
                root_link,
                root_model,
                root_table,
                models,
            )
        })
        .collect::<Result<Vec<_>, String>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
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
                #(#root_extra_path_methods)*
            }

            #(#scalar_fns)*
            #(#scalar_filter_fns)*
            #(#scalar_builder_modules)*
            #(#relation_modules)*
        }
    })
}

pub(super) fn wrappers_allow_ordering(wrappers: &[RelationPathSegment]) -> bool {
    wrappers
        .iter()
        .all(|segment| matches!(segment.kind, RelationFilterWrapperKind::ToOne))
}

#[allow(clippy::too_many_arguments)]
fn generate_relation_scalar_order_functions(
    current_model: &Model,
    model_names: &BTreeSet<&str>,
    root_link: &RelationLink,
    root_model: &Model,
    root_table: &str,
    path_prefix: &[String],
    models: &[Model],
    allow_ordering: bool,
) -> Result<Vec<proc_macro2::TokenStream>, String> {
    if !allow_ordering {
        return Ok(Vec::new());
    }

    scalar_model_fields(current_model, model_names)
        .into_iter()
        .map(|field| {
            let asc_ident = ident(&format!("{}_asc", field.name));
            let desc_ident = ident(&format!("{}_desc", field.name));
            let mut path = path_prefix.to_vec();
            path.push(field.name.clone());
            let value_sql = super::order_targets::relation_order_value_sql_for_path(
                root_model, models, root_table, &path,
            )?;
            let parent_table = root_link.parent_table.as_str();
            let parent_column = root_link.parent_column.as_str();
            let related_table = root_link.related_table.as_str();
            let related_column = root_link.related_column.as_str();

            Ok(quote! {
                #[allow(non_snake_case)]
                pub fn #asc_ident() -> ::cratestack::OrderClause {
                    ::cratestack::OrderClause::relation_scalar(
                        #parent_table,
                        #parent_column,
                        #related_table,
                        #related_column,
                        #value_sql,
                        ::cratestack::SortDirection::Asc,
                    )
                }

                #[allow(non_snake_case)]
                pub fn #desc_ident() -> ::cratestack::OrderClause {
                    ::cratestack::OrderClause::relation_scalar(
                        #parent_table,
                        #parent_column,
                        #related_table,
                        #related_column,
                        #value_sql,
                        ::cratestack::SortDirection::Desc,
                    )
                }
            })
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn generate_relation_scalar_builder_modules(
    current_model: &Model,
    model_names: &BTreeSet<&str>,
    wrappers: &[RelationPathSegment],
    allow_ordering: bool,
    root_link: &RelationLink,
    root_model: &Model,
    root_table: &str,
    path_prefix: &[String],
    models: &[Model],
) -> Result<Vec<proc_macro2::TokenStream>, String> {
    scalar_model_fields(current_model, model_names)
        .into_iter()
        .map(|field| {
            super::scalar_builder::generate_scalar_relation_builder_module(
                field,
                wrappers,
                allow_ordering,
                root_link,
                root_model,
                root_table,
                path_prefix,
                models,
            )
        })
        .collect()
}
