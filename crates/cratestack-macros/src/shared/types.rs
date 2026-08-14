//! Rust-type token generation + scalar parser tokens used by route
//! handlers when decoding query parameters.

use cratestack_core::{Field, TypeArity, TypeRef};
use quote::quote;

use super::{doc_attrs, ident};

#[cfg(test)]
mod tests;

pub(crate) fn rust_type_tokens(type_ref: &TypeRef) -> proc_macro2::TokenStream {
    rust_type_tokens_with_scope(type_ref, true)
}

pub(crate) fn rust_type_tokens_with_scope(
    type_ref: &TypeRef,
    custom_in_super: bool,
) -> proc_macro2::TokenStream {
    if type_ref.is_page() {
        let item = type_ref
            .page_item()
            .expect("validated Page<T> should include an item type");
        let item_type = rust_type_tokens_with_scope(item, custom_in_super);
        return quote! { ::cratestack::Page<#item_type> };
    }

    let inner = match type_ref.name.as_str() {
        "String" => quote! { String },
        "Cuid" => quote! { String },
        "Int" => quote! { i64 },
        "Float" => quote! { f64 },
        "Boolean" => quote! { bool },
        "DateTime" => quote! { ::cratestack::chrono::DateTime<::cratestack::chrono::Utc> },
        "Decimal" => crate::shared::decimal_backend::current_decimal_type_tokens(),
        "Json" => quote! { ::cratestack::Json<::cratestack::Value> },
        "Bytes" => quote! { Vec<u8> },
        "Uuid" => quote! { ::cratestack::uuid::Uuid },
        // `Vector(n)` (see `docs/design/extensions.md` §6) — a plain
        // `Vec<f32>` on the Rust side, kept ergonomic and dependency-
        // free (no `pgvector` crate reference in the public struct
        // field) regardless of which macro composer generates this:
        // server model, Create/Update input, or the Rust client's own
        // copy of the same struct shape. The `pgvector` crate only
        // enters the picture at the sqlx row-decode/bind boundary
        // (`model::row_pg`, `cratestack-sqlx`'s `push_bind_value`),
        // which is server-only.
        "Vector" => quote! { Vec<f32> },
        other => {
            let ident = ident(other);
            if custom_in_super {
                quote! { super::#ident }
            } else {
                quote! { #ident }
            }
        }
    };

    match type_ref.arity {
        TypeArity::Required => inner,
        TypeArity::Optional => quote! { Option<#inner> },
        TypeArity::List => quote! { Vec<#inner> },
    }
}

pub(crate) fn field_definition(
    field: &Field,
    wrap_for_patch: bool,
    custom_in_super: bool,
) -> proc_macro2::TokenStream {
    let field_ident = ident(&field.name);
    let docs = doc_attrs(&field.docs);
    let base_type = rust_type_tokens_with_scope(&field.ty, custom_in_super);
    let field_type = if wrap_for_patch {
        quote! { Option<#base_type> }
    } else {
        base_type
    };

    quote! {
        #docs
        pub #field_ident: #field_type,
    }
}

pub(crate) fn query_scalar_parser_tokens(
    ty: &TypeRef,
    value_expr: proc_macro2::TokenStream,
    field_name: &str,
) -> Option<proc_macro2::TokenStream> {
    Some(match ty.name.as_str() {
        "String" => quote! { Ok((#value_expr).to_owned()) },
        "Cuid" => quote! { ::cratestack::parse_cuid(#value_expr) },
        "Int" => quote! {
            (#value_expr).parse::<i64>().map_err(|error| {
                CoolError::BadRequest(format!("invalid value '{}' for {}: {error}", #value_expr, #field_name))
            })
        },
        "Float" => quote! {
            (#value_expr).parse::<f64>().map_err(|error| {
                CoolError::BadRequest(format!("invalid value '{}' for {}: {error}", #value_expr, #field_name))
            })
        },
        "Boolean" => quote! {
            (#value_expr).parse::<bool>().map_err(|error| {
                CoolError::BadRequest(format!("invalid value '{}' for {}: {error}", #value_expr, #field_name))
            })
        },
        "Uuid" => quote! {
            (#value_expr).parse::<::cratestack::uuid::Uuid>().map_err(|error| {
                CoolError::BadRequest(format!("invalid value '{}' for {}: {error}", #value_expr, #field_name))
            })
        },
        "DateTime" => quote! {
            (#value_expr)
                .parse::<::cratestack::chrono::DateTime<::cratestack::chrono::FixedOffset>>()
                .map(|value| value.with_timezone(&::cratestack::chrono::Utc))
                .map_err(|error| {
                    CoolError::BadRequest(format!("invalid value '{}' for {}: {error}", #value_expr, #field_name))
                })
        },
        "Decimal" => {
            let decimal_ty = crate::shared::decimal_backend::current_decimal_type_tokens();
            quote! {
                (#value_expr).parse::<#decimal_ty>().map_err(|error| {
                    CoolError::BadRequest(format!("invalid value '{}' for {}: {error}", #value_expr, #field_name))
                })
            }
        }
        _ => return None,
    })
}

pub(crate) fn query_scalar_list_parser_tokens(
    ty: &TypeRef,
    field_name: &str,
) -> Option<proc_macro2::TokenStream> {
    let scalar_parser = query_scalar_parser_tokens(ty, quote! { raw_value }, field_name)?;

    Some(quote! {{
        let parsed = value
            .split(',')
            .map(str::trim)
            .filter(|raw_value| !raw_value.is_empty())
            .map(|raw_value| -> Result<_, CoolError> { #scalar_parser })
            .collect::<Result<Vec<_>, CoolError>>()?;
        if parsed.is_empty() {
            return Err(CoolError::BadRequest(format!(
                "{}__in requires at least one value",
                #field_name,
            )));
        }
        parsed
    }})
}
