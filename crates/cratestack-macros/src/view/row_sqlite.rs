//! `impl FromRusqliteRow for <ViewName>` for embedded view rows.
//! Reuses the per-field decode emitter from
//! [`crate::model::row_sqlite::sqlite_row_field_tokens`] so view rows
//! decode every scalar identically to model rows.

use std::collections::BTreeSet;

use cratestack_core::View;
use quote::quote;

use crate::model::row_sqlite::sqlite_row_field_tokens;
use crate::shared::ident;

pub(crate) fn generate_view_rusqlite_from_row_impl(
    view: &View,
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    let view_ident = ident(&view.name);
    let row_fields = view
        .fields
        .iter()
        .map(|field| sqlite_row_field_tokens(field, enum_names));

    quote! {
        impl ::cratestack_rusqlite::FromRusqliteRow for #view_ident {
            fn from_rusqlite_row(
                row: &::cratestack_rusqlite::rusqlite::Row<'_>,
            ) -> ::cratestack_rusqlite::rusqlite::Result<Self> {
                Ok(Self {
                    #(#row_fields)*
                })
            }
        }
    }
}
