//! rusqlite row decoders. Embedded-side composer use only — must
//! not appear in server output.

use std::collections::BTreeSet;

use cratestack_core::{Field, Model, TypeArity};
use quote::quote;

use crate::shared::{ident, scalar_model_fields, to_snake_case};

/// Emit `impl FromRusqliteRow for Model` only.
/// **Embedded-side composer use only — must not appear in server output.**
pub(crate) fn generate_rusqlite_from_row_impl(
    model: &Model,
    model_names: &BTreeSet<&str>,
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    let model_ident = ident(&model.name);
    let scalar_fields = scalar_model_fields(model, model_names);
    let sqlite_row_fields = scalar_fields
        .iter()
        .map(|field| sqlite_row_field_tokens(field, enum_names));
    let partial_sqlite_row_fields = scalar_fields
        .iter()
        .map(|field| partial_sqlite_row_field_tokens(field, enum_names));

    quote! {
        impl ::cratestack_rusqlite::FromRusqliteRow for #model_ident {
            fn from_rusqlite_row(
                row: &::cratestack_rusqlite::rusqlite::Row<'_>,
            ) -> ::cratestack_rusqlite::rusqlite::Result<Self> {
                Ok(Self {
                    #(#sqlite_row_fields)*
                })
            }
        }

        impl ::cratestack_rusqlite::FromPartialRusqliteRow for #model_ident {
            fn from_partial_rusqlite_row(
                row: &::cratestack_rusqlite::rusqlite::Row<'_>,
                selected: &[&str],
            ) -> ::cratestack_rusqlite::rusqlite::Result<Self> {
                Ok(Self {
                    #(#partial_sqlite_row_fields)*
                })
            }
        }
    }
}

fn sqlite_row_field_tokens(field: &Field, enum_names: &BTreeSet<&str>) -> proc_macro2::TokenStream {
    let field_ident = ident(&field.name);
    let expr = sqlite_row_field_decode_expr(field, enum_names);
    quote! {
        #field_ident: #expr,
    }
}

/// Partial-decode mirror of [`sqlite_row_field_tokens`] — gates the
/// expression on whether the column was requested via `.select(...)`.
fn partial_sqlite_row_field_tokens(
    field: &Field,
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    let field_ident = ident(&field.name);
    let sql_name = to_snake_case(&field.name);
    let expr = sqlite_row_field_decode_expr(field, enum_names);
    quote! {
        #field_ident: if selected.iter().any(|c| *c == #sql_name) {
            #expr
        } else {
            ::std::default::Default::default()
        },
    }
}

fn sqlite_row_field_decode_expr(
    field: &Field,
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    let field_name = &field.name;

    // Enums round-trip as TEXT (same as the PG side).
    if enum_names.contains(field.ty.name.as_str()) {
        return enum_decode_expr(field, field_name);
    }

    // Scalar types: every type that's stored as TEXT on the device needs a
    // column-wrapper newtype so rusqlite's FromSql picks the right decoder.
    // Simple types (String, Int, Float, Bytes) use rusqlite's built-in FromSql
    // impls directly via `row.get(name)?`.
    match (field.ty.name.as_str(), field.ty.arity) {
        ("Boolean", TypeArity::Required) => quote! { row.get::<_, i64>(#field_name)? != 0 },
        ("Boolean", TypeArity::Optional) => quote! {
            row.get::<_, Option<i64>>(#field_name)?.map(|value| value != 0)
        },
        ("Uuid", TypeArity::Required) => {
            quote! { row.get::<_, ::cratestack::UuidColumn>(#field_name)?.0 }
        }
        ("Uuid", TypeArity::Optional) => quote! {
            row.get::<_, Option<::cratestack::UuidColumn>>(#field_name)?.map(|v| v.0)
        },
        ("DateTime", TypeArity::Required) => {
            quote! { row.get::<_, ::cratestack::DateTimeColumn>(#field_name)?.0 }
        }
        ("DateTime", TypeArity::Optional) => quote! {
            row.get::<_, Option<::cratestack::DateTimeColumn>>(#field_name)?.map(|v| v.0)
        },
        ("Decimal", TypeArity::Required) => {
            quote! { row.get::<_, ::cratestack::DecimalColumn>(#field_name)?.0 }
        }
        ("Decimal", TypeArity::Optional) => quote! {
            row.get::<_, Option<::cratestack::DecimalColumn>>(#field_name)?.map(|v| v.0)
        },
        ("Json", TypeArity::Required) => quote! {
            {
                let raw: String = row.get(#field_name)?;
                let value: ::cratestack::Value = ::serde_json::from_str(&raw)
                    .map_err(|error| ::cratestack::rusqlite::Error::FromSqlConversionFailure(
                        0,
                        ::cratestack::rusqlite::types::Type::Text,
                        Box::new(error),
                    ))?;
                ::cratestack::Json(value)
            }
        },
        ("Json", TypeArity::Optional) => quote! {
            {
                let raw: Option<String> = row.get(#field_name)?;
                match raw {
                    Some(text) => {
                        let value: ::cratestack::Value = ::serde_json::from_str(&text)
                            .map_err(|error| ::cratestack::rusqlite::Error::FromSqlConversionFailure(
                                0,
                                ::cratestack::rusqlite::types::Type::Text,
                                Box::new(error),
                            ))?;
                        Some(::cratestack::Json(value))
                    }
                    None => None,
                }
            }
        },
        // Default: rusqlite's built-in FromSql handles the conversion
        // (String, Int as i64, Float as f64, Bytes as Vec<u8>, Cuid as String).
        _ => quote! { row.get(#field_name)? },
    }
}

fn enum_decode_expr(field: &Field, field_name: &str) -> proc_macro2::TokenStream {
    let enum_ident = ident(&field.ty.name);
    let parse_error = |error: proc_macro2::TokenStream| {
        quote! {
            ::cratestack::rusqlite::Error::FromSqlConversionFailure(
                0,
                ::cratestack::rusqlite::types::Type::Text,
                Box::new(::std::io::Error::new(
                    ::std::io::ErrorKind::InvalidData,
                    #error,
                )),
            )
        }
    };
    match field.ty.arity {
        TypeArity::Required => {
            let decode_error = parse_error(quote! { error.to_string() });
            quote! {
                {
                    let raw: String = row.get(#field_name)?;
                    raw.parse::<super::types::#enum_ident>().map_err(|error| #decode_error)?
                }
            }
        }
        TypeArity::Optional => {
            let decode_error = parse_error(quote! { error.to_string() });
            quote! {
                {
                    let raw: Option<String> = row.get(#field_name)?;
                    raw.map(|value| value.parse::<super::types::#enum_ident>().map_err(|error| #decode_error)).transpose()?
                }
            }
        }
        TypeArity::List => {
            let decode_error = parse_error(quote! { error.to_string() });
            quote! {
                {
                    let raw: String = row.get(#field_name)?;
                    let strs: Vec<String> = ::serde_json::from_str(&raw).map_err(|error| #decode_error)?;
                    strs.into_iter()
                        .map(|value| value.parse::<super::types::#enum_ident>().map_err(|error| #decode_error))
                        .collect::<Result<Vec<_>, _>>()?
                }
            }
        }
    }
}
