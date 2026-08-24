//! Selection / list / fetch query DTOs emitted inside the axum
//! module. These are static (don't depend on the schema), so they
//! live in their own helper to keep [`super::axum_module`] tight.

use quote::quote;

pub(super) fn build_axum_dtos() -> proc_macro2::TokenStream {
    quote! {
        /// `{ fieldName: paramsJson }` — the decoded, key-validated shape
        /// of a `?computedParams=` query value. Values stay `serde_json::
        /// Value` here (not the generated params `type` struct) because
        /// this shape is schema-independent; per-field typed
        /// deserialization happens where the field's params type is
        /// known — inside `serialize_<model>_model_value` — not here.
        pub type ComputedParamsQuery = ::std::collections::BTreeMap<String, ::cratestack::serde_json::Value>;

        #[derive(Debug, Clone, Default)]
        pub struct ModelSelectionQuery {
            pub fields: Option<Vec<String>>,
            pub includes: Vec<String>,
            pub include_fields: ::std::collections::BTreeMap<String, Vec<String>>,
        }

        impl ModelSelectionQuery {
            fn fields_for_include(&self, include: &str) -> Option<&[String]> {
                self.include_fields.get(include).map(Vec::as_slice)
            }

            fn direct_includes(&self) -> Vec<String> {
                let mut includes = Vec::new();
                for include in &self.includes {
                    let direct = include.split('.').next().unwrap_or(include).to_owned();
                    if !includes.iter().any(|selected| selected == &direct) {
                        includes.push(direct);
                    }
                }
                includes
            }

            fn selection_for_include(&self, include: &str) -> Option<Self> {
                let mut selection = Self::default();
                if let Some(fields) = self.include_fields.get(include) {
                    selection.fields = Some(fields.clone());
                }

                let prefix = format!("{include}.");
                for selected in &self.includes {
                    if let Some(rest) = selected.strip_prefix(&prefix) {
                        selection.includes.push(rest.to_owned());
                    }
                }
                for (path, fields) in &self.include_fields {
                    if let Some(rest) = path.strip_prefix(&prefix) {
                        selection.include_fields.insert(rest.to_owned(), fields.clone());
                    }
                }

                if self.includes.iter().any(|selected| selected == include)
                    || selection.fields.is_some()
                    || !selection.includes.is_empty()
                {
                    Some(selection)
                } else {
                    None
                }
            }
        }

        #[derive(Debug, Clone, Default)]
        pub struct ModelListQuery {
            pub selection: ModelSelectionQuery,
            pub limit: Option<i64>,
            pub offset: Option<i64>,
            pub sort: Option<String>,
            pub filters: Vec<::cratestack::QueryExpr>,
            /// Raw `?computedParams=` value, not yet decoded or validated
            /// — per-model validation needs the model's computed-field
            /// list, which isn't available at this schema-independent
            /// parse site. See `parse_<model>_computed_params`.
            pub computed_params: Option<String>,
        }

        #[derive(Debug, Clone, Default)]
        pub struct ModelFetchQuery {
            pub selection: ModelSelectionQuery,
            /// See `ModelListQuery::computed_params`.
            pub computed_params: Option<String>,
        }
    }
}
