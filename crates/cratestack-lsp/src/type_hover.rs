//! Hover for a named type: a field's or argument's model, `type` or enum,
//! and an enum's own declaration and variants. An ordinal enum
//! (`Pending = 1`, `docs/design/enum-ordinals.md`) shows each variant's
//! number, which is what the column and the wire carry.

use cratestack_core::{EnumDecl, Schema, TypeRef};

use crate::state::SymbolInfo;
use crate::text::span_contains;
use crate::type_ref::type_ref_at_offset;

/// An enum's name or one of its variants, at `offset`.
pub(crate) fn enum_symbol(schema: &Schema, offset: usize) -> Option<SymbolInfo> {
    let decl = schema
        .enums
        .iter()
        .find(|e| span_contains(e.span, offset))?;
    if let Some(variant) = decl.variants.iter().find(|v| span_contains(v.span, offset)) {
        let detail = match variant.ordinal {
            Some(ordinal) => format!("{}.{} = {ordinal}", decl.name, variant.name),
            None => format!("variant of {}", decl.name),
        };
        return Some(SymbolInfo {
            kind: "enum variant",
            name: variant.name.clone(),
            detail,
            docs: variant.docs.clone(),
            selection_span: variant.span,
        });
    }
    span_contains(decl.name_span, offset).then(|| enum_decl_symbol(decl))
}

fn enum_decl_symbol(decl: &EnumDecl) -> SymbolInfo {
    let detail = if decl.is_ordinal() {
        let variants = decl
            .variants
            .iter()
            .filter_map(|v| Some(format!("{} = {}", v.name, v.ordinal?)))
            .collect::<Vec<_>>()
            .join(", ");
        match &decl.reserved {
            Some(reserved) => {
                let retired = reserved
                    .values
                    .iter()
                    .map(i64::to_string)
                    .collect::<Vec<_>>();
                format!("ordinal enum: {variants}; reserved {}", retired.join(", "))
            }
            None => format!("ordinal enum: {variants}"),
        }
    } else {
        let names = decl
            .variants
            .iter()
            .map(|v| v.name.as_str())
            .collect::<Vec<_>>();
        format!("enum: {}", names.join(" | "))
    };
    SymbolInfo {
        kind: "enum",
        name: decl.name.clone(),
        detail,
        docs: decl.docs.clone(),
        selection_span: decl.name_span,
    }
}

pub(crate) fn named_type_symbol(
    schema: &Schema,
    ty: &TypeRef,
    offset: usize,
) -> Option<SymbolInfo> {
    if let Some(inner) = ty
        .generic_args
        .iter()
        .find(|inner| type_ref_at_offset(inner, offset))
    {
        return named_type_symbol(schema, inner, offset);
    }
    schema
        .models
        .iter()
        .find(|model| model.name == ty.name)
        .map(|model| SymbolInfo {
            kind: "model",
            name: model.name.clone(),
            detail: "model".to_owned(),
            docs: model.docs.clone(),
            selection_span: model.name_span,
        })
        .or_else(|| {
            schema
                .types
                .iter()
                .find(|decl| decl.name == ty.name)
                .map(|decl| SymbolInfo {
                    kind: "type",
                    name: decl.name.clone(),
                    detail: "type".to_owned(),
                    docs: decl.docs.clone(),
                    selection_span: decl.name_span,
                })
        })
        .or_else(|| {
            let decl = schema.enums.iter().find(|decl| decl.name == ty.name)?;
            Some(enum_decl_symbol(decl))
        })
}
