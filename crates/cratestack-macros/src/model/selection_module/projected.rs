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

        /// Per-arity fallback for a JSON object that omits the
        /// projected field's key. See [`decode_projected_field`].
        #[derive(Debug, Clone, Copy)]
        enum MissingFieldFallback {
            /// Required arity — missing key is a hard
            /// `CoolError::Internal("missing field …")`.
            Reject,
            /// Optional arity — missing key is treated as
            /// `Value::Null`, which serde maps to `Option::None`.
            Null,
            /// List arity — missing key is treated as
            /// `Value::Array(vec![])`. Serde refuses to
            /// deserialize `Vec<T>` from `null`, and the
            /// `#[serde(default)]` on the model field only fires
            /// at whole-struct deserialization time, so we have
            /// to produce an empty JSON array explicitly.
            EmptyArray,
        }

        /// Decode one projected scalar out of a `Projected`'s JSON
        /// object.
        ///
        /// The whole point of per-arity fallbacks is that adding a
        /// new optional / list projection field to a server is no
        /// longer a breaking change for clients (real or mocked)
        /// that haven't been updated to include the field in their
        /// payloads — the decoder degrades to `None` / `Vec::new()`
        /// instead of failing the whole round-trip with a
        /// "missing field" error. Required-arity strictness is
        /// preserved so a route that forgets to project the
        /// primary key still surfaces.
        fn decode_projected_field<T>(
            object: &::cratestack::serde_json::Map<String, ::cratestack::serde_json::Value>,
            selected: bool,
            model_name: &str,
            field_name: &str,
            fallback: MissingFieldFallback,
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

            let value = match (object.get(field_name).cloned(), fallback) {
                (Some(value), _) => value,
                (None, MissingFieldFallback::Null) => {
                    ::cratestack::serde_json::Value::Null
                }
                (None, MissingFieldFallback::EmptyArray) => {
                    ::cratestack::serde_json::Value::Array(::std::vec::Vec::new())
                }
                (None, MissingFieldFallback::Reject) => {
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
