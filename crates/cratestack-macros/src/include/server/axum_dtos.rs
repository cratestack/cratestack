//! Selection / list / fetch query DTOs emitted inside the axum
//! module. These are static (don't depend on the schema), so they
//! live in their own helper to keep [`super::axum_module`] tight.

use quote::quote;

pub(super) fn build_axum_dtos() -> proc_macro2::TokenStream {
    quote! {
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
        }

        #[derive(Debug, Clone, Default)]
        pub struct ModelFetchQuery {
            pub selection: ModelSelectionQuery,
        }
    }
}
