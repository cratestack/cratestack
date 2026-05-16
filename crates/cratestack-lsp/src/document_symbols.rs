use cratestack_core::{ProcedureKind, Schema, SourceSpan};
use tower_lsp_server::ls_types::{DocumentSymbol, SymbolKind};

use crate::text::range_from_offsets;
use crate::type_ref::render_type_ref;

pub(crate) fn document_symbols(text: &str, schema: &Schema) -> Vec<DocumentSymbol> {
    let mut symbols = Vec::new();

    if let Some(datasource) = &schema.datasource {
        symbols.push(document_symbol_leaf(
            text,
            datasource.name.clone(),
            SymbolKind::OBJECT,
            Some("datasource".to_owned()),
            datasource.span,
            datasource.span,
        ));
    }

    if let Some(auth) = &schema.auth {
        symbols.push(document_symbol_with_children(
            text,
            auth.name.clone(),
            SymbolKind::OBJECT,
            Some("auth".to_owned()),
            auth.span,
            auth.span,
            auth.fields
                .iter()
                .map(|field| field_document_symbol(text, field))
                .collect(),
        ));
    }

    symbols.extend(schema.models.iter().map(|model| {
        document_symbol_with_children(
            text,
            model.name.clone(),
            SymbolKind::STRUCT,
            Some("model".to_owned()),
            model.span,
            model.name_span,
            model
                .fields
                .iter()
                .map(|field| field_document_symbol(text, field))
                .collect(),
        )
    }));

    symbols.extend(schema.types.iter().map(|ty| {
        document_symbol_with_children(
            text,
            ty.name.clone(),
            SymbolKind::CLASS,
            Some("type".to_owned()),
            ty.span,
            ty.name_span,
            ty.fields
                .iter()
                .map(|field| field_document_symbol(text, field))
                .collect(),
        )
    }));

    symbols.extend(schema.procedures.iter().map(|procedure| {
        let detail = match procedure.kind {
            ProcedureKind::Query => "procedure".to_owned(),
            ProcedureKind::Mutation => "mutation procedure".to_owned(),
        };
        let mut children = procedure
            .args
            .iter()
            .map(|arg| {
                document_symbol_leaf(
                    text,
                    arg.name.clone(),
                    SymbolKind::VARIABLE,
                    Some(render_type_ref(&arg.ty)),
                    arg.span,
                    arg.name_span,
                )
            })
            .collect::<Vec<_>>();
        children.sort_by_key(|symbol| (symbol.range.start.line, symbol.range.start.character));
        document_symbol_with_children(
            text,
            procedure.name.clone(),
            SymbolKind::FUNCTION,
            Some(detail),
            procedure.span,
            procedure.name_span,
            children,
        )
    }));

    symbols
}

fn field_document_symbol(text: &str, field: &cratestack_core::Field) -> DocumentSymbol {
    document_symbol_leaf(
        text,
        field.name.clone(),
        SymbolKind::FIELD,
        Some(render_type_ref(&field.ty)),
        field.span,
        field.name_span,
    )
}

#[allow(deprecated)]
fn document_symbol_with_children(
    text: &str,
    name: String,
    kind: SymbolKind,
    detail: Option<String>,
    span: SourceSpan,
    selection_span: SourceSpan,
    children: Vec<DocumentSymbol>,
) -> DocumentSymbol {
    let range = range_from_offsets(text, span.start, span.end);
    let selection_range = range_from_offsets(text, selection_span.start, selection_span.end);
    DocumentSymbol {
        name,
        detail,
        kind,
        tags: None,
        deprecated: None,
        range,
        selection_range,
        children: Some(children),
    }
}

#[allow(deprecated)]
fn document_symbol_leaf(
    text: &str,
    name: String,
    kind: SymbolKind,
    detail: Option<String>,
    span: SourceSpan,
    selection_span: SourceSpan,
) -> DocumentSymbol {
    let range = range_from_offsets(text, span.start, span.end);
    let selection_range = range_from_offsets(text, selection_span.start, selection_span.end);
    DocumentSymbol {
        name,
        detail,
        kind,
        tags: None,
        deprecated: None,
        range,
        selection_range,
        children: None,
    }
}
