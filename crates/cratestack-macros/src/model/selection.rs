use std::collections::BTreeSet;

use cratestack_core::{Field, Model, TypeArity};
use quote::quote;

use crate::shared::{ident, relation_model_fields, rust_type_tokens, to_snake_case};

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
            quote! {
                #[allow(non_snake_case)]
                pub fn #method_ident(&self) -> Result<#field_type, ::cratestack::CoolError> {
                    decode_projected_field::<#field_type>(
                        &self.fields,
                        self.allows_field(#field_name),
                        #model_name,
                        #field_name,
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

fn build_selection_include_accessor(
    field: &Field,
    model_name: &str,
    include_name: &str,
    include_field_ident: &proc_macro2::Ident,
    target_module_ident: &proc_macro2::Ident,
) -> proc_macro2::TokenStream {
    if field.ty.arity == TypeArity::List {
        return quote! {
            #[allow(non_snake_case)]
            pub fn #include_field_ident(
                &self,
            ) -> Result<Vec<super::super::#target_module_ident::selection::ProjectedInclude>, ::cratestack::CoolError> {
                let selection = self.selection.includes.#include_field_ident.as_ref().ok_or_else(|| {
                    ::cratestack::CoolError::Validation(format!(
                        "include '{}' was not selected for {}",
                        #include_name,
                        #model_name,
                    ))
                })?;
                let value = self.fields.get(#include_name).cloned().ok_or_else(|| {
                    ::cratestack::CoolError::Internal(format!(
                        "projected {} payload is missing include '{}'",
                        #model_name,
                        #include_name,
                    ))
                })?;
                match value {
                    ::cratestack::serde_json::Value::Array(values) => values
                        .into_iter()
                        .map(|value| {
                            super::super::#target_module_ident::selection::ProjectedInclude::from_value(
                                value,
                                selection.as_ref().clone(),
                            )
                        })
                        .collect(),
                    other => Err(::cratestack::CoolError::Internal(format!(
                        "projected include '{}.{}' must be an array, got {other:?}",
                        #model_name,
                        #include_name,
                    ))),
                }
            }
        };
    }

    quote! {
        #[allow(non_snake_case)]
        pub fn #include_field_ident(
            &self,
        ) -> Result<Option<super::super::#target_module_ident::selection::ProjectedInclude>, ::cratestack::CoolError> {
            let selection = self.selection.includes.#include_field_ident.as_ref().ok_or_else(|| {
                ::cratestack::CoolError::Validation(format!(
                    "include '{}' was not selected for {}",
                    #include_name,
                    #model_name,
                ))
            })?;
            let value = self.fields.get(#include_name).cloned().ok_or_else(|| {
                ::cratestack::CoolError::Internal(format!(
                    "projected {} payload is missing include '{}'",
                    #model_name,
                    #include_name,
                ))
            })?;
            match value {
                ::cratestack::serde_json::Value::Null => Ok(None),
                other => super::super::#target_module_ident::selection::ProjectedInclude::from_value(
                    other,
                    selection.as_ref().clone(),
                )
                .map(Some),
            }
        }
    }
}
