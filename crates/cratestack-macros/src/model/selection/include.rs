//! Accessor methods for projecting an included relation back out of a
//! `ProjectedInclude`. List vs scalar arity produce different return
//! types (`Vec<...>` vs `Option<...>`).

use cratestack_core::{Field, TypeArity};
use quote::quote;

pub(super) fn build_selection_include_accessor(
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
