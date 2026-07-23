//! sqlx Postgres row decoders: `FromRow` + `FromPartialPgRow` impl
//! tokens, plus the per-field decode helpers they share. Server-only —
//! must not appear in embedded output.

use std::collections::BTreeSet;

use cratestack_core::{Field, Model, TypeArity};
use quote::quote;

use crate::shared::{ident, scalar_model_fields, to_snake_case};

/// Emit `impl sqlx::FromRow<'_, sqlx::postgres::PgRow> for Model` only.
/// **Server-side composer use only — must not appear in embedded output.**
pub(crate) fn generate_pg_from_row_impl(
    model: &Model,
    model_names: &BTreeSet<&str>,
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    let model_ident = ident(&model.name);
    let scalar_fields = scalar_model_fields(model, model_names);
    let row_fields = scalar_fields
        .iter()
        .map(|field| row_field_tokens(field, enum_names));
    let partial_row_fields = scalar_fields
        .iter()
        .map(|field| partial_row_field_tokens(field, enum_names));

    quote! {
        impl<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> for #model_ident {
            fn from_row(row: &'r sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
                use sqlx::Row;

                Ok(Self {
                    #(#row_fields)*
                })
            }
        }

        impl ::cratestack::FromPartialPgRow for #model_ident {
            fn decode_partial_pg_row(
                row: &sqlx::postgres::PgRow,
                selected: &[&str],
            ) -> ::std::result::Result<Self, sqlx::Error> {
                use sqlx::Row;
                Ok(Self {
                    #(#partial_row_fields)*
                })
            }
        }
    }
}

/// Partial-decode variant of [`row_field_tokens`]. Emits the same
/// per-field decode expression but gated on whether the column was
/// requested via `.select(...)`. The `selected` slice carries the
/// SQL column names (snake_case) so we match against
/// `to_snake_case(&field.name)` rather than the Rust-side field name.
fn partial_row_field_tokens(
    field: &Field,
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    let field_ident = ident(&field.name);
    let sql_name = to_snake_case(&field.name);
    let decode_expr = row_field_decode_expr(field, enum_names);
    quote! {
        #field_ident: if selected.iter().any(|c| *c == #sql_name) {
            #decode_expr
        } else {
            ::std::default::Default::default()
        },
    }
}

/// Extract the decode expression for a single field — same shape as
/// the body of [`row_field_tokens`] but without the `field_ident:`
/// prefix and trailing comma, so it can plug into the conditional
/// branch of [`partial_row_field_tokens`].
fn row_field_decode_expr(field: &Field, enum_names: &BTreeSet<&str>) -> proc_macro2::TokenStream {
    let field_name = &field.name;

    // Special-case JSON columns: decode through DbJson so plain JSONB is
    // accepted and then convert into the public Json<Value> wrapper.
    if field.ty.name == "Json" {
        match field.ty.arity {
            TypeArity::Required => {
                return quote! {{
                    let raw: ::cratestack::sqlx::types::Json<::cratestack::DbJson> = row.try_get(#field_name)?;
                    ::cratestack::sqlx::types::Json(raw.0.into())
                }};
            }
            TypeArity::Optional => {
                return quote! {{
                    let raw: Option<::cratestack::sqlx::types::Json<::cratestack::DbJson>> = row.try_get(#field_name)?;
                    raw.map(|r| ::cratestack::sqlx::types::Json(r.0.into()))
                }};
            }
            TypeArity::List => {
                return quote! {{
                    let raw: Vec<::cratestack::sqlx::types::Json<::cratestack::DbJson>> = row.try_get(#field_name)?;
                    raw.into_iter().map(|r| ::cratestack::sqlx::types::Json(r.0.into())).collect::<Vec<_>>()
                }};
            }
        }
    }

    if !enum_names.contains(field.ty.name.as_str()) {
        return quote! { row.try_get(#field_name)? };
    }

    let enum_ident = ident(&field.ty.name);
    let parse_error = |error: proc_macro2::TokenStream| {
        quote! {
            sqlx::Error::Decode(Box::new(::std::io::Error::new(
                ::std::io::ErrorKind::InvalidData,
                #error,
            )))
        }
    };
    match field.ty.arity {
        TypeArity::Required => {
            let decode_error = parse_error(quote! { error });
            quote! {
                {
                    let raw: String = row.try_get(#field_name)?;
                    raw.parse::<super::types::#enum_ident>().map_err(|error| #decode_error)?
                }
            }
        }
        TypeArity::Optional => {
            let decode_error = parse_error(quote! { error });
            quote! {
                {
                    let raw: Option<String> = row.try_get(#field_name)?;
                    raw.map(|value| value.parse::<super::types::#enum_ident>().map_err(|error| #decode_error)).transpose()?
                }
            }
        }
        TypeArity::List => {
            let decode_error = parse_error(quote! { error });
            quote! {
                {
                    let raw: Vec<String> = row.try_get(#field_name)?;
                    raw.into_iter()
                        .map(|value| value.parse::<super::types::#enum_ident>().map_err(|error| #decode_error))
                        .collect::<Result<Vec<_>, sqlx::Error>>()?
                }
            }
        }
    }
}

fn row_field_tokens(field: &Field, enum_names: &BTreeSet<&str>) -> proc_macro2::TokenStream {
    let field_ident = ident(&field.name);
    let field_name = &field.name;

    // Special-case JSON columns: decode through DbJson and convert into
    // the public Json<Value> wrapper so the stored plain JSONB is accepted.
    if field.ty.name == "Json" {
        match field.ty.arity {
            TypeArity::Required => {
                return quote! {
                    #field_ident: {
                        let raw: ::cratestack::sqlx::types::Json<::cratestack::DbJson> = row.try_get(#field_name)?;
                        ::cratestack::sqlx::types::Json(raw.0.into())
                    },
                };
            }
            TypeArity::Optional => {
                return quote! {
                    #field_ident: {
                        let raw: Option<::cratestack::sqlx::types::Json<::cratestack::DbJson>> = row.try_get(#field_name)?;
                        raw.map(|r| ::cratestack::sqlx::types::Json(r.0.into()))
                    },
                };
            }
            TypeArity::List => {
                return quote! {
                    #field_ident: {
                        let raw: Vec<::cratestack::sqlx::types::Json<::cratestack::DbJson>> = row.try_get(#field_name)?;
                        raw.into_iter().map(|r| ::cratestack::sqlx::types::Json(r.0.into())).collect::<Vec<_>>()
                    },
                };
            }
        }
    }

    if !enum_names.contains(field.ty.name.as_str()) {
        return quote! {
            #field_ident: row.try_get(#field_name)?,
        };
    }

    let enum_ident = ident(&field.ty.name);
    let parse_error = |error: proc_macro2::TokenStream| {
        quote! {
            sqlx::Error::Decode(Box::new(::std::io::Error::new(
                ::std::io::ErrorKind::InvalidData,
                #error,
            )))
        }
    };

    match field.ty.arity {
        TypeArity::Required => {
            let decode_error = parse_error(quote! { error });
            quote! {
                #field_ident: {
                    let raw: String = row.try_get(#field_name)?;
                    raw.parse::<super::types::#enum_ident>().map_err(|error| #decode_error)?
                },
            }
        }
        TypeArity::Optional => {
            let decode_error = parse_error(quote! { error });
            quote! {
                #field_ident: {
                    let raw: Option<String> = row.try_get(#field_name)?;
                    raw.map(|value| value.parse::<super::types::#enum_ident>().map_err(|error| #decode_error)).transpose()?
                },
            }
        }
        TypeArity::List => {
            let decode_error = parse_error(quote! { error });
            quote! {
                #field_ident: {
                    let raw: Vec<String> = row.try_get(#field_name)?;
                    raw.into_iter()
                        .map(|value| value.parse::<super::types::#enum_ident>().map_err(|error| #decode_error))
                        .collect::<Result<Vec<_>, sqlx::Error>>()?
                },
            }
        }
    }
}
