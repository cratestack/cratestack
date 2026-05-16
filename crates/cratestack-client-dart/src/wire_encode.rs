use std::collections::BTreeSet;

use cratestack_core::{TypeArity, TypeRef};

fn synthetic_span() -> cratestack_core::SourceSpan {
    cratestack_core::SourceSpan {
        start: 0,
        end: 0,
        line: 1,
    }
}

pub(crate) fn encode_value_expr(
    expr: &str,
    ty: &TypeRef,
    enum_names: &BTreeSet<&str>,
    force_nullable: bool,
) -> String {
    match ty.arity {
        TypeArity::List => {
            let item = encode_required_scalar(
                "item",
                &TypeRef {
                    name: ty.name.clone(),
                    name_span: synthetic_span(),
                    arity: TypeArity::Required,
                    generic_args: ty.generic_args.clone(),
                },
                enum_names,
            );
            if force_nullable {
                format!("{expr}?.map((item) => {item}).toList(growable: false)")
            } else {
                format!("{expr}.map((item) => {item}).toList(growable: false)")
            }
        }
        TypeArity::Optional => encode_optional_scalar(expr, ty, enum_names),
        TypeArity::Required => {
            if force_nullable {
                encode_optional_scalar(
                    expr,
                    &TypeRef {
                        name: ty.name.clone(),
                        name_span: synthetic_span(),
                        arity: TypeArity::Optional,
                        generic_args: ty.generic_args.clone(),
                    },
                    enum_names,
                )
            } else {
                encode_required_scalar(expr, ty, enum_names)
            }
        }
    }
}

fn encode_required_scalar(expr: &str, ty: &TypeRef, enum_names: &BTreeSet<&str>) -> String {
    if ty.is_page() {
        return format!("{expr}.toWire()");
    }

    if enum_names.contains(ty.name.as_str()) {
        return format!("{expr}.toWire()");
    }

    match ty.name.as_str() {
        "DateTime" => format!("{expr}.toUtc().toIso8601String()"),
        "Bytes" => format!("{expr}.toList(growable: false)"),
        "Json" | "String" | "Cuid" | "Uuid" | "Int" | "Float" | "Boolean" => expr.to_owned(),
        _ => format!("{expr}.toWire()"),
    }
}

fn encode_optional_scalar(expr: &str, ty: &TypeRef, enum_names: &BTreeSet<&str>) -> String {
    if ty.is_page() {
        return format!("{expr}?.toWire()");
    }

    if enum_names.contains(ty.name.as_str()) {
        return format!("{expr}?.toWire()");
    }

    match ty.name.as_str() {
        "DateTime" => format!("{expr}?.toUtc().toIso8601String()"),
        "Bytes" => format!("{expr}?.toList(growable: false)"),
        "Json" | "String" | "Cuid" | "Uuid" | "Int" | "Float" | "Boolean" => expr.to_owned(),
        _ => format!("{expr}?.toWire()"),
    }
}
