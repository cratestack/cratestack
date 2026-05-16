use cratestack_core::{ProcedureKind, Schema, TypeRef};

use crate::state::SymbolInfo;
use crate::text::span_contains;
use crate::type_ref::{render_type_ref, type_ref_at_offset};

pub(crate) fn locate_symbol(schema: &Schema, offset: usize) -> Option<SymbolInfo> {
    if let Some(datasource) = &schema.datasource
        && span_contains(datasource.span, offset)
    {
        return Some(SymbolInfo {
            kind: "datasource",
            name: datasource.name.clone(),
            detail: "datasource block".to_owned(),
            docs: datasource.docs.clone(),
            selection_span: datasource.span,
        });
    }

    if let Some(auth) = &schema.auth {
        for field in &auth.fields {
            if span_contains(field.ty.name_span, offset)
                && let Some(symbol) = named_type_symbol(schema, &field.ty, offset)
            {
                return Some(symbol);
            }
            if span_contains(field.span, offset) {
                return Some(field_symbol(field));
            }
        }
        if span_contains(auth.span, offset) {
            return Some(SymbolInfo {
                kind: "auth",
                name: auth.name.clone(),
                detail: "auth block".to_owned(),
                docs: auth.docs.clone(),
                selection_span: auth.span,
            });
        }
    }

    for model in &schema.models {
        for field in &model.fields {
            if span_contains(field.ty.name_span, offset)
                && let Some(symbol) = named_type_symbol(schema, &field.ty, offset)
            {
                return Some(symbol);
            }
            if span_contains(field.span, offset) {
                return Some(field_symbol(field));
            }
        }
        if span_contains(model.name_span, offset) {
            return Some(SymbolInfo {
                kind: "model",
                name: model.name.clone(),
                detail: "model".to_owned(),
                docs: model.docs.clone(),
                selection_span: model.name_span,
            });
        }
    }

    for ty in &schema.types {
        for field in &ty.fields {
            if span_contains(field.ty.name_span, offset)
                && let Some(symbol) = named_type_symbol(schema, &field.ty, offset)
            {
                return Some(symbol);
            }
            if span_contains(field.span, offset) {
                return Some(field_symbol(field));
            }
        }
        if span_contains(ty.name_span, offset) {
            return Some(SymbolInfo {
                kind: "type",
                name: ty.name.clone(),
                detail: "type".to_owned(),
                docs: ty.docs.clone(),
                selection_span: ty.name_span,
            });
        }
    }

    for procedure in &schema.procedures {
        for arg in &procedure.args {
            if span_contains(arg.ty.name_span, offset)
                && let Some(symbol) = named_type_symbol(schema, &arg.ty, offset)
            {
                return Some(symbol);
            }
            if span_contains(arg.span, offset) {
                return Some(SymbolInfo {
                    kind: "argument",
                    name: arg.name.clone(),
                    detail: render_type_ref(&arg.ty),
                    docs: arg.docs.clone(),
                    selection_span: arg.name_span,
                });
            }
        }
        if type_ref_at_offset(&procedure.return_type, offset)
            && let Some(symbol) = named_type_symbol(schema, &procedure.return_type, offset)
        {
            return Some(symbol);
        }
        if span_contains(procedure.name_span, offset) {
            let kind = match procedure.kind {
                ProcedureKind::Query => "procedure",
                ProcedureKind::Mutation => "mutation procedure",
            };
            let detail = format!("{} -> {}", kind, render_type_ref(&procedure.return_type));
            return Some(SymbolInfo {
                kind,
                name: procedure.name.clone(),
                detail,
                docs: procedure.docs.clone(),
                selection_span: procedure.name_span,
            });
        }
    }

    None
}

fn field_symbol(field: &cratestack_core::Field) -> SymbolInfo {
    SymbolInfo {
        kind: "field",
        name: field.name.clone(),
        detail: render_type_ref(&field.ty),
        docs: field.docs.clone(),
        selection_span: field.name_span,
    }
}

fn named_type_symbol(schema: &Schema, ty: &TypeRef, offset: usize) -> Option<SymbolInfo> {
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
}
