//! Generic `Value` token generation used by `ProcedureArgs::procedure_arg_value`
//! and similar shape-agnostic value projections.

use std::collections::BTreeSet;

use cratestack_core::{TypeArity, TypeRef};
use quote::quote;

pub(crate) fn value_tokens(
    value: proc_macro2::TokenStream,
    ty: &TypeRef,
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    if enum_names.contains(ty.name.as_str()) {
        return match ty.arity {
            TypeArity::Required => quote! { ::cratestack::Value::String(#value.to_string()) },
            TypeArity::Optional => quote! {
                match #value {
                    Some(value) => ::cratestack::Value::String(value.to_string()),
                    None => ::cratestack::Value::Null,
                }
            },
            TypeArity::List => quote! {
                ::cratestack::Value::List(
                    #value
                        .into_iter()
                        .map(|value| ::cratestack::Value::String(value.to_string()))
                        .collect()
                )
            },
        };
    }

    match (ty.name.as_str(), ty.arity) {
        ("String", TypeArity::Required) | ("Cuid", TypeArity::Required) => {
            quote! { ::cratestack::Value::String(#value) }
        }
        ("String", TypeArity::Optional) | ("Cuid", TypeArity::Optional) => quote! {
            match #value {
                Some(value) => ::cratestack::Value::String(value),
                None => ::cratestack::Value::Null,
            }
        },
        ("Int", TypeArity::Required) => quote! { ::cratestack::Value::Int(#value) },
        ("Int", TypeArity::Optional) => quote! {
            match #value {
                Some(value) => ::cratestack::Value::Int(value),
                None => ::cratestack::Value::Null,
            }
        },
        ("Boolean", TypeArity::Required) => quote! { ::cratestack::Value::Bool(#value) },
        ("Boolean", TypeArity::Optional) => quote! {
            match #value {
                Some(value) => ::cratestack::Value::Bool(value),
                None => ::cratestack::Value::Null,
            }
        },
        _ => quote! { ::cratestack::Value::Null },
    }
}
