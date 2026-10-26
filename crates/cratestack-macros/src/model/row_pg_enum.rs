//! Decoding an enum column from a Postgres row, shared by model rows
//! (`row_pg`) and view and `query` rows (`view::row_pg`) so the two cannot
//! disagree about how an enum column decodes. Server-only.
//!
//! `__CRATESTACK_ORDINAL` is a `const` on the generated enum type
//! (`types/enums_ordinal.rs`), so one arm is dead code: an ordinal enum's
//! column is a `SMALLINT`, a string enum's is `TEXT`.

use cratestack_core::TypeArity;
use quote::quote;

use crate::shared::ident;

pub(crate) fn pg_enum_decode_expr(
    field_name: &str,
    enum_name: &str,
    arity: TypeArity,
) -> proc_macro2::TokenStream {
    let enum_ty = {
        let enum_ident = ident(enum_name);
        quote! { super::types::#enum_ident }
    };
    let decode_error = quote! {
        sqlx::Error::Decode(Box::new(::std::io::Error::new(
            ::std::io::ErrorKind::InvalidData,
            error,
        )))
    };
    match arity {
        TypeArity::Required => quote! {
            {
                if #enum_ty::__CRATESTACK_ORDINAL {
                    let raw: i16 = row.try_get(#field_name)?;
                    #enum_ty::__cratestack_from_ordinal(i64::from(raw))
                        .map_err(|error| #decode_error)?
                } else {
                    let raw: String = row.try_get(#field_name)?;
                    raw.parse::<#enum_ty>().map_err(|error| #decode_error)?
                }
            }
        },
        TypeArity::Optional => quote! {
            {
                if #enum_ty::__CRATESTACK_ORDINAL {
                    let raw: Option<i16> = row.try_get(#field_name)?;
                    raw.map(|value| {
                        #enum_ty::__cratestack_from_ordinal(i64::from(value))
                            .map_err(|error| #decode_error)
                    })
                    .transpose()?
                } else {
                    let raw: Option<String> = row.try_get(#field_name)?;
                    raw.map(|value| value.parse::<#enum_ty>().map_err(|error| #decode_error))
                        .transpose()?
                }
            }
        },
        // Unreachable in practice as of #229: `cratestack-parser` rejects a
        // list-arity enum model field on any `datasource`-bearing schema
        // before this macro expands. Kept because `TypeArity` is matched
        // exhaustively across the codegen surface; it is also wrong on its
        // own terms (a `Vec<String>` against a scalar `TEXT`/CHECK column,
        // since #233), which real list support end to end would fix.
        TypeArity::List => quote! {
            {
                let raw: Vec<String> = row.try_get(#field_name)?;
                raw.into_iter()
                    .map(|value| value.parse::<#enum_ty>().map_err(|error| #decode_error))
                    .collect::<Result<Vec<_>, sqlx::Error>>()?
            }
        },
    }
}
