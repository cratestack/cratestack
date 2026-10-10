//! Scalar parser tokens used by route handlers when decoding query
//! parameters (`?where=` style filters). Split out of `types.rs` per the
//! repo's 200-LoC file convention.
//!
//! `None` means the field is not query-filterable (`Json`, `Bytes`, the
//! extension scalars, a custom `type`): [`query_scalar_parser_tokens`] is
//! therefore a gate as well as a parser, and a scalar that falls through to
//! `None` silently drops its filter key. `tests_bigint` holds a guard that
//! fails when a built-in scalar lands there without being named on purpose.

use std::collections::BTreeSet;

use cratestack_core::TypeRef;
use quote::quote;

use super::super::enum_query_parser::query_enum_parser_tokens;

pub(crate) fn query_scalar_parser_tokens(
    ty: &TypeRef,
    value_expr: proc_macro2::TokenStream,
    field_name: &str,
    enum_names: &BTreeSet<&str>,
) -> Option<proc_macro2::TokenStream> {
    // Issue #928: an enum-typed field is a first-class query-filter
    // scalar too — checked ahead of the fixed catch-all match below
    // since `ty.name` is schema-authored and can't collide with one of
    // the builtin scalar names matched there.
    if enum_names.contains(ty.name.as_str()) {
        return Some(query_enum_parser_tokens(ty, value_expr, field_name));
    }

    Some(match ty.name.as_str() {
        "String" => quote! { Ok((#value_expr).to_owned()) },
        "Cuid" => quote! { ::cratestack::parse_cuid(#value_expr) },
        "Int" => quote! {
            (#value_expr).parse::<i64>().map_err(|error| {
                CratestackError::BadRequest(format!("invalid value '{}' for {}: {error}", #value_expr, #field_name))
            })
        },
        // The canonical decimal grammar only (`FromStr` for `BigInt`), the
        // same one the body codecs enforce: `+5`, `007`, `-0` and a value
        // outside `i64` are a 400 here exactly as they are in a JSON or CBOR
        // body, rather than being read leniently by `i64::from_str`.
        "BigInt" => quote! {
            (#value_expr).parse::<::cratestack::BigInt>().map_err(|error| {
                CratestackError::BadRequest(format!("invalid value '{}' for {}: {error}", #value_expr, #field_name))
            })
        },
        "Float" => quote! {
            (#value_expr).parse::<f64>().map_err(|error| {
                CratestackError::BadRequest(format!("invalid value '{}' for {}: {error}", #value_expr, #field_name))
            })
        },
        "Boolean" => quote! {
            (#value_expr).parse::<bool>().map_err(|error| {
                CratestackError::BadRequest(format!("invalid value '{}' for {}: {error}", #value_expr, #field_name))
            })
        },
        "Uuid" => quote! {
            (#value_expr).parse::<::cratestack::uuid::Uuid>().map_err(|error| {
                CratestackError::BadRequest(format!("invalid value '{}' for {}: {error}", #value_expr, #field_name))
            })
        },
        "DateTime" => quote! {
            (#value_expr)
                .parse::<::cratestack::chrono::DateTime<::cratestack::chrono::FixedOffset>>()
                .map(|value| value.with_timezone(&::cratestack::chrono::Utc))
                .map_err(|error| {
                    CratestackError::BadRequest(format!("invalid value '{}' for {}: {error}", #value_expr, #field_name))
                })
        },
        "Decimal" => {
            let decimal_ty = crate::shared::decimal_backend::current_decimal_type_tokens();
            quote! {
                (#value_expr).parse::<#decimal_ty>().map_err(|error| {
                    CratestackError::BadRequest(format!("invalid value '{}' for {}: {error}", #value_expr, #field_name))
                })
            }
        }
        _ => return None,
    })
}

pub(crate) fn query_scalar_list_parser_tokens(
    ty: &TypeRef,
    field_name: &str,
    enum_names: &BTreeSet<&str>,
) -> Option<proc_macro2::TokenStream> {
    let scalar_parser =
        query_scalar_parser_tokens(ty, quote! { raw_value }, field_name, enum_names)?;

    Some(quote! {{
        let parsed = value
            .split(',')
            .map(str::trim)
            .filter(|raw_value| !raw_value.is_empty())
            .map(|raw_value| -> Result<_, CratestackError> { #scalar_parser })
            .collect::<Result<Vec<_>, CratestackError>>()?;
        if parsed.is_empty() {
            return Err(CratestackError::BadRequest(format!(
                "{}__in requires at least one value",
                #field_name,
            )));
        }
        parsed
    }})
}
