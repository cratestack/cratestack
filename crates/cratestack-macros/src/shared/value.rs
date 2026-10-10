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
        // The in-process value a procedure policy compares (`args.owner != 7`);
        // it is never serialized to a client. `Value` has no 64-bit-string
        // form and `Value::Int` is `i64`, so a `BigInt` is its `i64`: the same
        // variant a literal (`ProcedurePolicyLiteral::Int`) and an integer
        // auth claim compare against. Falling to `Value::Null` here is the
        // fail-open this arm exists to prevent: `!=` against a null passes.
        ("BigInt", TypeArity::Required) => quote! { ::cratestack::Value::Int(#value.get()) },
        ("BigInt", TypeArity::Optional) => quote! {
            match #value {
                Some(value) => ::cratestack::Value::Int(value.get()),
                None => ::cratestack::Value::Null,
            }
        },
        ("BigInt", TypeArity::List) => quote! {
            ::cratestack::Value::List(
                #value
                    .into_iter()
                    .map(|value| ::cratestack::Value::Int(value.get()))
                    .collect()
            )
        },
        ("Boolean", TypeArity::Required) => quote! { ::cratestack::Value::Bool(#value) },
        ("Boolean", TypeArity::Optional) => quote! {
            match #value {
                Some(value) => ::cratestack::Value::Bool(value),
                None => ::cratestack::Value::Null,
            }
        },
        // Everything else has no policy-comparable form. A policy literal is
        // only ever a `Boolean`, an integer or a `String`
        // (`policy::procedure::resolver::parse_procedure_literal`), but two
        // arguments of the same type can still be compared with each other,
        // and two `Null`s are equal. `tests_values` lists every built-in
        // scalar that reaches this arm on purpose, and fails when a new one
        // does, so that adding a scalar is a decision here and not an accident.
        _ => quote! { ::cratestack::Value::Null },
    }
}

#[cfg(test)]
mod tests_values;
