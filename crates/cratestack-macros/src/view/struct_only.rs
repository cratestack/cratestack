//! Emit the `pub struct <ViewName> { fields }` for a `view` block.
//!
//! Mirrors [`crate::model::struct_only::generate_model_struct_only`]
//! but takes a [`cratestack_core::View`] and skips the relation-field
//! filter (views don't carry relation fields in v1 — `@from` resolves
//! to a source-model scalar, and relation-follow off a view is
//! deferred per ADR-0003 §"Deferred").

use std::collections::BTreeSet;

use cratestack_core::View;
use quote::quote;

use crate::model::struct_only::struct_field_definition;
use crate::shared::{doc_attrs, ident};

pub(crate) fn generate_view_struct_only(
    view: &View,
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    let view_ident = ident(&view.name);
    let docs = doc_attrs(&view.docs);
    let fields = view
        .fields
        .iter()
        .map(|field| struct_field_definition(field, false, enum_names));

    // `Default` matches the model-side rationale (`Projection<T>` needs
    // it for non-selected fields). `serde::{Serialize, Deserialize}`
    // round-trip the view rows through the generated client and the
    // RPC dispatcher.
    quote! {
        #docs
        #[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
        pub struct #view_ident {
            #(#fields)*
        }
    }
}
