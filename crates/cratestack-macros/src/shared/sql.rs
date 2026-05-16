//! `SqlValue` token generation for create / update input fields.

use std::collections::BTreeSet;

use cratestack_core::{Field, TypeArity, TypeRef};
use quote::quote;

use super::{ident, to_snake_case};

pub(crate) fn create_sql_value(
    field: &Field,
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    let field_ident = ident(&field.name);
    let column = to_snake_case(&field.name);
    let value = sql_value_tokens(quote! { self.#field_ident.clone() }, &field.ty, enum_names);

    quote! {
        ::cratestack::SqlColumnValue {
            column: #column,
            value: #value,
        }
    }
}

pub(crate) fn update_sql_value(
    field: &Field,
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    let field_ident = ident(&field.name);
    let column = to_snake_case(&field.name);
    let some_value = sql_value_tokens(quote! { value }, &field.ty, enum_names);

    quote! {
        if let Some(value) = self.#field_ident.clone() {
            values.push(::cratestack::SqlColumnValue {
                column: #column,
                value: #some_value,
            });
        }
    }
}

pub(crate) fn sql_value_tokens(
    value: proc_macro2::TokenStream,
    ty: &TypeRef,
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    if enum_names.contains(ty.name.as_str()) {
        return match ty.arity {
            TypeArity::Required => quote! { ::cratestack::SqlValue::String(#value.to_string()) },
            TypeArity::Optional => quote! {
                match #value {
                    Some(value) => ::cratestack::SqlValue::String(value.to_string()),
                    None => ::cratestack::SqlValue::NullString,
                }
            },
            TypeArity::List => panic!("unsupported SQLx enum list type for this slice"),
        };
    }

    match (ty.name.as_str(), ty.arity) {
        ("String", TypeArity::Required) | ("Cuid", TypeArity::Required) => {
            quote! { ::cratestack::SqlValue::String(#value) }
        }
        ("String", TypeArity::Optional) | ("Cuid", TypeArity::Optional) => quote! {
            match #value {
                Some(value) => ::cratestack::SqlValue::String(value),
                None => ::cratestack::SqlValue::NullString,
            }
        },
        ("Int", TypeArity::Required) => quote! { ::cratestack::SqlValue::Int(#value) },
        ("Int", TypeArity::Optional) => quote! {
            match #value {
                Some(value) => ::cratestack::SqlValue::Int(value),
                None => ::cratestack::SqlValue::NullInt,
            }
        },
        ("Float", TypeArity::Required) => quote! { ::cratestack::SqlValue::Float(#value) },
        ("Float", TypeArity::Optional) => quote! {
            match #value {
                Some(value) => ::cratestack::SqlValue::Float(value),
                None => ::cratestack::SqlValue::NullFloat,
            }
        },
        ("Boolean", TypeArity::Required) => quote! { ::cratestack::SqlValue::Bool(#value) },
        ("Boolean", TypeArity::Optional) => quote! {
            match #value {
                Some(value) => ::cratestack::SqlValue::Bool(value),
                None => ::cratestack::SqlValue::NullBool,
            }
        },
        ("Bytes", TypeArity::Required) => quote! { ::cratestack::SqlValue::Bytes(#value) },
        ("Bytes", TypeArity::Optional) => quote! {
            match #value {
                Some(value) => ::cratestack::SqlValue::Bytes(value),
                None => ::cratestack::SqlValue::NullBytes,
            }
        },
        ("Uuid", TypeArity::Required) => quote! { ::cratestack::SqlValue::Uuid(#value) },
        ("Uuid", TypeArity::Optional) => quote! {
            match #value {
                Some(value) => ::cratestack::SqlValue::Uuid(value),
                None => ::cratestack::SqlValue::NullUuid,
            }
        },
        ("DateTime", TypeArity::Required) => quote! { ::cratestack::SqlValue::DateTime(#value) },
        ("DateTime", TypeArity::Optional) => quote! {
            match #value {
                Some(value) => ::cratestack::SqlValue::DateTime(value),
                None => ::cratestack::SqlValue::NullDateTime,
            }
        },
        ("Json", TypeArity::Required) => quote! { ::cratestack::SqlValue::Json(#value.0) },
        ("Json", TypeArity::Optional) => quote! {
            match #value {
                Some(value) => ::cratestack::SqlValue::Json(value.0),
                None => ::cratestack::SqlValue::NullJson,
            }
        },
        ("Decimal", TypeArity::Required) => quote! { ::cratestack::SqlValue::Decimal(#value) },
        ("Decimal", TypeArity::Optional) => quote! {
            match #value {
                Some(value) => ::cratestack::SqlValue::Decimal(value),
                None => ::cratestack::SqlValue::NullDecimal,
            }
        },
        _ => panic!("unsupported SQLx value type for this slice"),
    }
}
