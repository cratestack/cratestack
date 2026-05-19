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

        /// Decode one projected scalar out of a `Projected`'s JSON
        /// object.
        ///
        /// `is_optional` switches the missing-field behaviour:
        ///
        /// - `false` (required arity) — a JSON object missing the
        ///   key is a hard payload error. Callers receive
        ///   `CoolError::Internal` and the read fails. This matches
        ///   the original strict behaviour.
        /// - `true` (Optional / List arity) — a missing key is
        ///   treated as `null`. `serde_json::from_value(Null)`
        ///   resolves `Option<T>` to `None` and (via
        ///   `#[serde(default)]` on the model field) `Vec<T>` to
        ///   `Vec::new()`. This is what consumers want: when a
        ///   server adds a new optional projection field, existing
        ///   client mocks / older responses that omit the field
        ///   keep decoding as `None` instead of failing the whole
        ///   round-trip with a "missing field" error.
        fn decode_projected_field<T>(
            object: &::cratestack::serde_json::Map<String, ::cratestack::serde_json::Value>,
            selected: bool,
            model_name: &str,
            field_name: &str,
            is_optional: bool,
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

            let value = match object.get(field_name).cloned() {
                Some(value) => value,
                None if is_optional => ::cratestack::serde_json::Value::Null,
                None => {
                    return Err(::cratestack::CoolError::Internal(format!(
                        "projected {} payload is missing field '{}'",
                        model_name,
                        field_name,
                    )));
                }
            };

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
