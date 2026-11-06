//! Decoding an enum column from a SQLite row (`row_sqlite`). Every SQLite
//! column is declared `BLOB`; a string enum's value is stored as text and an
//! ordinal enum's as an integer. `__CRATESTACK_ORDINAL` is a `const` on the
//! generated type (`types/enums_ordinal.rs`), so one arm is dead code.

use cratestack_core::{Field, TypeArity};
use quote::quote;

use crate::shared::ident;

pub(super) fn enum_decode_expr(field: &Field, field_name: &str) -> proc_macro2::TokenStream {
    let enum_ident = ident(&field.ty.name);
    let conversion_error = |ty: proc_macro2::TokenStream, error: proc_macro2::TokenStream| {
        quote! {
            ::cratestack::rusqlite::Error::FromSqlConversionFailure(
                0,
                ::cratestack::rusqlite::types::Type::#ty,
                Box::new(::std::io::Error::new(
                    ::std::io::ErrorKind::InvalidData,
                    #error,
                )),
            )
        }
    };
    let text_error = conversion_error(quote! { Text }, quote! { error.to_string() });
    let int_error = conversion_error(quote! { Integer }, quote! { error });
    match field.ty.arity {
        TypeArity::Required => quote! {
            {
                if super::types::#enum_ident::__CRATESTACK_ORDINAL {
                    let raw: i64 = row.get(#field_name)?;
                    super::types::#enum_ident::__cratestack_from_ordinal(raw)
                        .map_err(|error| #int_error)?
                } else {
                    let raw: String = row.get(#field_name)?;
                    raw.parse::<super::types::#enum_ident>().map_err(|error| #text_error)?
                }
            }
        },
        TypeArity::Optional => quote! {
            {
                if super::types::#enum_ident::__CRATESTACK_ORDINAL {
                    let raw: Option<i64> = row.get(#field_name)?;
                    raw.map(|value| {
                        super::types::#enum_ident::__cratestack_from_ordinal(value)
                            .map_err(|error| #int_error)
                    })
                    .transpose()?
                } else {
                    let raw: Option<String> = row.get(#field_name)?;
                    raw.map(|value| value.parse::<super::types::#enum_ident>().map_err(|error| #text_error))
                        .transpose()?
                }
            }
        },
        // Unreachable as of #229; see the matching note in `row_pg_enum.rs`.
        // No write-side counterpart either: `cratestack-rusqlite`'s
        // `ToSql` (`src/value/bind.rs`) has no array case.
        TypeArity::List => quote! {
            {
                let raw: String = row.get(#field_name)?;
                let strs: Vec<String> = ::serde_json::from_str(&raw).map_err(|error| #text_error)?;
                strs.into_iter()
                    .map(|value| value.parse::<super::types::#enum_ident>().map_err(|error| #text_error))
                    .collect::<Result<Vec<_>, _>>()?
            }
        },
    }
}
