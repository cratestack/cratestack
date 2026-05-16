//! `Projected` + `ProjectedInclude` structs — decoded views of a row
//! that gate scalar accessors on the original selection's `fields` set
//! and forward relation accessors into nested `ProjectedInclude`s.

use quote::quote;

pub(super) fn build_projected_block(
    model_name: &str,
    selected_scalar_accessors: &[proc_macro2::TokenStream],
    included_scalar_accessors: &[proc_macro2::TokenStream],
    include_accessors: &[proc_macro2::TokenStream],
) -> proc_macro2::TokenStream {
    quote! {
        #[derive(Debug, Clone)]
        pub struct Projected {
            fields: ::cratestack::serde_json::Map<String, ::cratestack::serde_json::Value>,
            selection: Selection,
        }

        impl Projected {
            fn from_value(
                value: ::cratestack::serde_json::Value,
                selection: Selection,
            ) -> Result<Self, ::cratestack::CoolError> {
                match value {
                    ::cratestack::serde_json::Value::Object(fields) => Ok(Self { fields, selection }),
                    other => Err(::cratestack::CoolError::Internal(format!(
                        "projected {} payload must be an object, got {other:?}",
                        #model_name,
                    ))),
                }
            }

            fn allows_field(&self, field: &str) -> bool {
                match &self.selection.fields {
                    Some(fields) => fields.contains(field),
                    None => true,
                }
            }

            pub fn raw(&self) -> &::cratestack::serde_json::Map<String, ::cratestack::serde_json::Value> {
                &self.fields
            }

            #(#selected_scalar_accessors)*
            #(#include_accessors)*
        }

        #[derive(Debug, Clone)]
        pub struct ProjectedInclude {
            fields: ::cratestack::serde_json::Map<String, ::cratestack::serde_json::Value>,
            selection: IncludeSelection,
        }

        impl ProjectedInclude {
            pub(crate) fn from_value(
                value: ::cratestack::serde_json::Value,
                selection: IncludeSelection,
            ) -> Result<Self, ::cratestack::CoolError> {
                match value {
                    ::cratestack::serde_json::Value::Object(fields) => Ok(Self { fields, selection }),
                    other => Err(::cratestack::CoolError::Internal(format!(
                        "projected included {} payload must be an object, got {other:?}",
                        #model_name,
                    ))),
                }
            }

            fn allows_field(&self, field: &str) -> bool {
                match &self.selection.fields {
                    Some(fields) => fields.contains(field),
                    None => true,
                }
            }

            pub fn raw(&self) -> &::cratestack::serde_json::Map<String, ::cratestack::serde_json::Value> {
                &self.fields
            }

            #(#included_scalar_accessors)*
            #(#include_accessors)*
        }

        fn decode_projected_field<T>(
            object: &::cratestack::serde_json::Map<String, ::cratestack::serde_json::Value>,
            selected: bool,
            model_name: &str,
            field_name: &str,
        ) -> Result<T, ::cratestack::CoolError>
        where
            T: ::cratestack::serde::de::DeserializeOwned,
        {
            if !selected {
                return Err(::cratestack::CoolError::Validation(format!(
                    "field '{}.{}' was not selected",
                    model_name,
                    field_name,
                )));
            }

            let value = object.get(field_name).cloned().ok_or_else(|| {
                ::cratestack::CoolError::Internal(format!(
                    "projected {} payload is missing field '{}'",
                    model_name,
                    field_name,
                ))
            })?;

            ::cratestack::serde_json::from_value(value).map_err(|error| {
                ::cratestack::CoolError::Internal(format!(
                    "failed to decode projected field '{}.{}': {error}",
                    model_name,
                    field_name,
                ))
            })
        }
    }
}
