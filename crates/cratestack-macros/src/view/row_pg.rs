//! `impl sqlx::FromRow<'_, sqlx::postgres::PgRow> for <ViewName>` for
//! views (ADR-0003). Server-only — must not appear in embedded output.
//!
//! Each view field decodes by its camelCase `rust_name` because the
//! generated `SELECT` (via `ViewDescriptor::select_projection`)
//! emits `<sql_name> AS "<rust_name>"`. Enum fields parse from their
//! string representation, same as model rows.

use std::collections::BTreeSet;

use cratestack_core::{TypeArity, View};
use quote::quote;

use crate::shared::ident;

pub(crate) fn generate_view_pg_from_row_impl(
    view: &View,
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    let view_ident = ident(&view.name);
    let row_fields = view
        .fields
        .iter()
        .map(|field| row_field_tokens(field, enum_names));

    quote! {
        impl<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> for #view_ident {
            fn from_row(row: &'r sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
                use sqlx::Row;
                Ok(Self {
                    #(#row_fields)*
                })
            }
        }
    }
}

fn row_field_tokens(
    field: &cratestack_core::Field,
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    let field_ident = ident(&field.name);
    let field_name = &field.name;
    if !enum_names.contains(field.ty.name.as_str()) {
        return quote! {
            #field_ident: row.try_get(#field_name)?,
        };
    }
    let enum_ident = ident(&field.ty.name);
    let decode_error = quote! {
        sqlx::Error::Decode(Box::new(::std::io::Error::new(
            ::std::io::ErrorKind::InvalidData,
            error,
        )))
    };
    match field.ty.arity {
        TypeArity::Required => quote! {
            #field_ident: {
                let raw: String = row.try_get(#field_name)?;
                raw.parse::<super::types::#enum_ident>().map_err(|error| #decode_error)?
            },
        },
        TypeArity::Optional => quote! {
            #field_ident: {
                let raw: Option<String> = row.try_get(#field_name)?;
                raw.map(|value| value.parse::<super::types::#enum_ident>().map_err(|error| #decode_error)).transpose()?
            },
        },
        TypeArity::List => quote! {
            #field_ident: {
                let raw: Vec<String> = row.try_get(#field_name)?;
                raw.into_iter()
                    .map(|value| value.parse::<super::types::#enum_ident>().map_err(|error| #decode_error))
                    .collect::<Result<Vec<_>, sqlx::Error>>()?
            },
        },
    }
}
