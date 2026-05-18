//! `Selection` + `Projection` impl + `IncludeSelection`. Both
//! selection types share the same `fields`/`includes` shape plus the
//! generated per-field setters; `Selection` adds the
//! `Projection`-trait decode hooks while `IncludeSelection` is
//! query-only.

use quote::quote;

pub(super) fn build_selection_block(
    model_name: &str,
    selection_field_methods: &[proc_macro2::TokenStream],
    include_selection_field_methods: &[proc_macro2::TokenStream],
    include_methods: &[proc_macro2::TokenStream],
    include_query_steps: &[proc_macro2::TokenStream],
) -> proc_macro2::TokenStream {
    quote! {
        #[derive(Debug, Clone, Default)]
        pub struct Selection {
            fields: Option<::std::collections::BTreeSet<&'static str>>,
            includes: Includes,
        }

        impl Selection {
            pub fn all_fields(mut self) -> Self {
                self.fields = None;
                self
            }

            #(#selection_field_methods)*
            #(#include_methods)*

            pub fn to_query(&self) -> ::cratestack::SelectionQuery {
                let mut query = ::cratestack::SelectionQuery::default();
                if let Some(fields) = &self.fields {
                    query.fields = fields.iter().map(|field| (*field).to_owned()).collect();
                }
                #(#include_query_steps)*
                query
            }

            pub fn decode_one(
                &self,
                value: ::cratestack::serde_json::Value,
            ) -> Result<Projected, ::cratestack::CoolError> {
                Projected::from_value(value, self.clone())
            }

            pub fn decode_many(
                &self,
                value: ::cratestack::serde_json::Value,
            ) -> Result<Vec<Projected>, ::cratestack::CoolError> {
                match value {
                    ::cratestack::serde_json::Value::Array(values) => values
                        .into_iter()
                        .map(|value| self.decode_one(value))
                        .collect(),
                    other => Err(::cratestack::CoolError::Internal(format!(
                        "projected {} list payload must be an array, got {other:?}",
                        #model_name,
                    ))),
                }
            }

            pub fn decode_page(
                &self,
                value: ::cratestack::serde_json::Value,
            ) -> Result<::cratestack::Page<Projected>, ::cratestack::CoolError> {
                let page = ::cratestack::serde_json::from_value::<::cratestack::Page<::cratestack::serde_json::Value>>(value)
                    .map_err(|error| ::cratestack::CoolError::Codec(format!(
                        "failed to decode projected {} page payload: {error}",
                        #model_name,
                    )))?;
                let items = page
                    .items
                    .into_iter()
                    .map(|value| self.decode_one(value))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(::cratestack::Page::new(items, page.page_info).with_total_count(page.total_count))
            }
        }

        impl ::cratestack::ProjectionDecoder for Selection {
            type Output = Projected;

            fn selection_query(&self) -> ::cratestack::SelectionQuery {
                self.to_query()
            }

            fn decode_one(
                &self,
                value: ::cratestack::serde_json::Value,
            ) -> Result<Self::Output, ::cratestack::CoolError> {
                Selection::decode_one(self, value)
            }

            fn decode_many(
                &self,
                value: ::cratestack::serde_json::Value,
            ) -> Result<Vec<Self::Output>, ::cratestack::CoolError> {
                Selection::decode_many(self, value)
            }

            fn decode_page(
                &self,
                value: ::cratestack::serde_json::Value,
            ) -> Result<::cratestack::Page<Self::Output>, ::cratestack::CoolError> {
                Selection::decode_page(self, value)
            }
        }

        #[derive(Debug, Clone, Default)]
        pub struct IncludeSelection {
            fields: Option<::std::collections::BTreeSet<&'static str>>,
            includes: Includes,
        }

        impl IncludeSelection {
            pub fn all_fields(mut self) -> Self {
                self.fields = None;
                self
            }

            #(#include_selection_field_methods)*
            #(#include_methods)*

            pub fn to_query(&self) -> ::cratestack::SelectionQuery {
                let mut query = ::cratestack::SelectionQuery::default();
                if let Some(fields) = &self.fields {
                    query.fields = fields.iter().map(|field| (*field).to_owned()).collect();
                }
                #(#include_query_steps)*
                query
            }
        }
    }
}
