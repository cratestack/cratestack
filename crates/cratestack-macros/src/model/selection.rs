mod include;

use std::collections::BTreeSet;

use cratestack_core::{Field, Model};
use quote::quote;

use crate::shared::{ident, relation_model_fields, rust_type_tokens, to_snake_case};

use include::build_selection_include_accessor;

pub(super) struct SelectionRelationEntry {
    pub(super) include_methods: proc_macro2::TokenStream,
    pub(super) include_field: proc_macro2::TokenStream,
    pub(super) include_query_step: proc_macro2::TokenStream,
    pub(super) include_accessor: proc_macro2::TokenStream,
}

pub(super) fn build_selection_field_methods(fields: &[&Field]) -> Vec<proc_macro2::TokenStream> {
    fields
        .iter()
        .map(|field| {
            let method_ident = ident(&field.name);
            let field_name = &field.name;
            quote! {
                #[allow(non_snake_case)]
                pub fn #method_ident(mut self) -> Self {
                    self.fields
                        .get_or_insert_with(::std::collections::BTreeSet::new)
                        .insert(#field_name);
                    self
                }
            }
        })
        .collect()
}

pub(super) fn build_selected_scalar_accessors(
    fields: &[&Field],
    model_name: &str,
) -> Vec<proc_macro2::TokenStream> {
    fields
        .iter()
        .map(|field| {
            let method_ident = ident(&field.name);
            let field_name = &field.name;
            let field_type = rust_type_tokens(&field.ty);
            // Optional / List arity gets the "missing field tolerated"
            // path in `decode_projected_field` — a JSON object that
            // omits the key is treated as if the key were present
            // with a `null` value, which serde then turns into `None`
            // for `Option<T>` and `Vec::new()` for `Vec<T>` (via the
            // `#[serde(default)]` attribute on the model struct's
            // field). Required arity stays strict — a missing
            // required field is a hard payload error, same as before.
            let is_optional = matches!(
                field.ty.arity,
                cratestack_core::TypeArity::Optional | cratestack_core::TypeArity::List
            );
            quote! {
                #[allow(non_snake_case)]
                pub fn #method_ident(&self) -> Result<#field_type, ::cratestack::CoolError> {
                    decode_projected_field::<#field_type>(
                        &self.fields,
                        self.allows_field(#field_name),
                        #model_name,
                        #field_name,
                        #is_optional,
                    )
                }
            }
        })
        .collect()
}

pub(super) fn build_selection_relation_entries(
    model: &Model,
    model_names: &BTreeSet<&str>,
    model_name: &str,
) -> Result<Vec<SelectionRelationEntry>, String> {
    relation_model_fields(model, model_names)
        .into_iter()
        .map(|field| build_selection_relation_entry(field, model_name))
        .collect()
}

fn build_selection_relation_entry(
    field: &Field,
    model_name: &str,
) -> Result<SelectionRelationEntry, String> {
    let include_method_ident = ident(&format!("include_{}", field.name));
    let include_selected_method_ident = ident(&format!("include_{}_selected", field.name));
    let include_name = field.name.clone();
    let include_field_ident = ident(&field.name);
    let target_module_ident = ident(&to_snake_case(&field.ty.name));
    let target_include_selection =
        quote! { super::super::#target_module_ident::selection::IncludeSelection };

    let include_methods = quote! {
        #[allow(non_snake_case)]
        pub fn #include_method_ident(mut self) -> Self {
            self.includes.#include_field_ident = Some(Box::new(#target_include_selection::default()));
            self
        }

        #[allow(non_snake_case)]
        pub fn #include_selected_method_ident(
            mut self,
            selection: #target_include_selection,
        ) -> Self {
            self.includes.#include_field_ident = Some(Box::new(selection));
            self
        }
    };
    let include_field =
        quote! { pub #include_field_ident: Option<Box<#target_include_selection>>, };
    let include_query_step = quote! {
        if let Some(selection) = &self.includes.#include_field_ident {
            let prefix = #include_name;
            query.includes.push(prefix.to_owned());
            let include_query = selection.to_query();
            if !include_query.fields.is_empty() {
                query.include_fields.insert(prefix.to_owned(), include_query.fields);
            }
            for nested_include in include_query.includes {
                query.includes.push(format!("{prefix}.{nested_include}"));
            }
            for (path, fields) in include_query.include_fields {
                query.include_fields.insert(format!("{prefix}.{path}"), fields);
            }
        }
    };
    let include_accessor = build_selection_include_accessor(
        field,
        model_name,
        &include_name,
        &include_field_ident,
        &target_module_ident,
    );

    Ok(SelectionRelationEntry {
        include_methods,
        include_field,
        include_query_step,
        include_accessor,
    })
}
