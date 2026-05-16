use std::collections::BTreeSet;

use cratestack_core::{TypeArity, TypeRef};

use crate::dart_types::dart_type;

fn synthetic_span() -> cratestack_core::SourceSpan {
    cratestack_core::SourceSpan {
        start: 0,
        end: 0,
        line: 1,
    }
}

pub(crate) fn decode_value_expr(
    expr: &str,
    ty: &TypeRef,
    enum_names: &BTreeSet<&str>,
    force_nullable: bool,
    owner_name: &str,
    field_name: &str,
) -> String {
    match ty.arity {
        TypeArity::List => {
            if force_nullable {
                let item = decode_required_scalar(
                    "item",
                    &TypeRef {
                        name: ty.name.clone(),
                        name_span: synthetic_span(),
                        arity: TypeArity::Required,
                        generic_args: ty.generic_args.clone(),
                    },
                    enum_names,
                );
                format!(
                    "{expr} == null ? null : cratestackAsValueList({expr}).map((item) => {item}).toList(growable: false)"
                )
            } else {
                let item = decode_required_scalar(
                    "item",
                    &TypeRef {
                        name: ty.name.clone(),
                        name_span: synthetic_span(),
                        arity: TypeArity::Required,
                        generic_args: ty.generic_args.clone(),
                    },
                    enum_names,
                );
                let list_expr =
                    format!("cratestackRequireWireValue('{owner_name}', '{field_name}', {expr})");
                format!(
                    "cratestackAsValueList({list_expr}).map((item) => {item}).toList(growable: false)"
                )
            }
        }
        TypeArity::Optional => decode_optional_scalar(expr, ty, enum_names),
        TypeArity::Required => {
            if force_nullable {
                decode_optional_scalar(
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
                let required_expr =
                    format!("cratestackRequireWireValue('{owner_name}', '{field_name}', {expr})");
                decode_required_scalar(&required_expr, ty, enum_names)
            }
        }
    }
}

fn decode_required_scalar(expr: &str, ty: &TypeRef, enum_names: &BTreeSet<&str>) -> String {
    if ty.is_page() {
        let item = ty
            .page_item()
            .expect("validated Page<T> should include an item type");
        let item_decode = decode_required_scalar("item", item, enum_names);
        return format!(
            "Page<{}>.fromWire(cratestackAsValueMap({expr}), decodeItem: (item) => {item_decode})",
            dart_type(item, false),
        );
    }

    if enum_names.contains(ty.name.as_str()) {
        return format!("{}.fromWire({expr})", ty.name);
    }

    match ty.name.as_str() {
        "String" | "Cuid" | "Uuid" => format!("{expr} as String"),
        "Int" => format!("({expr} as num).toInt()"),
        "Float" => format!("({expr} as num).toDouble()"),
        "Boolean" => format!("{expr} as bool"),
        "DateTime" => format!("DateTime.parse({expr} as String)"),
        "Json" => expr.to_owned(),
        "Bytes" => format!("Uint8List.fromList(List<int>.from(cratestackAsValueList({expr})))"),
        other => format!("{other}.fromWire(cratestackAsValueMap({expr}))"),
    }
}

fn decode_optional_scalar(expr: &str, ty: &TypeRef, enum_names: &BTreeSet<&str>) -> String {
    if ty.name == "Json" {
        return expr.to_owned();
    }

    let required = decode_required_scalar(
        expr,
        &TypeRef {
            name: ty.name.clone(),
            name_span: synthetic_span(),
            arity: TypeArity::Required,
            generic_args: ty.generic_args.clone(),
        },
        enum_names,
    );
    format!("{expr} == null ? null : {required}")
}
