use cratestack_core::ProcedureKind;
use tower_lsp_server::ls_types::{DocumentSymbol, SymbolKind};

use crate::state::DocumentState;
use crate::symbol_build::{
    document_symbol_leaf, document_symbol_with_children, field_document_symbol,
};
use crate::type_ref::render_type_ref;
use crate::view::Resolved;

/// Symbols for a document, or `None` when nothing has parsed yet.
pub(crate) fn document_symbols_for(document: &DocumentState) -> Option<Vec<DocumentSymbol>> {
    Some(document_symbols_in(&document.view()?))
}

#[cfg(test)]
pub(crate) fn document_symbols(
    text: &str,
    schema: &cratestack_core::Schema,
) -> Vec<DocumentSymbol> {
    document_symbols_in(&Resolved::single(text, schema))
}

/// The outline of the requesting file: declarations in other files of a split
/// schema are not in it.
fn document_symbols_in(view: &Resolved<'_>) -> Vec<DocumentSymbol> {
    let schema = view.schema;
    let mut symbols = Vec::new();

    if let Some(datasource) = &schema.datasource {
        symbols.extend(document_symbol_leaf(
            view,
            datasource.name.clone(),
            SymbolKind::OBJECT,
            Some("datasource".to_owned()),
            datasource.span,
            datasource.span,
        ));
    }

    if let Some(auth) = &schema.auth {
        symbols.extend(document_symbol_with_children(
            view,
            auth.name.clone(),
            SymbolKind::OBJECT,
            Some("auth".to_owned()),
            auth.span,
            auth.span,
            auth.fields
                .iter()
                .filter_map(|field| field_document_symbol(view, field))
                .collect(),
        ));
    }

    symbols.extend(schema.models.iter().filter_map(|model| {
        document_symbol_with_children(
            view,
            model.name.clone(),
            SymbolKind::STRUCT,
            Some("model".to_owned()),
            model.span,
            model.name_span,
            model
                .fields
                .iter()
                .filter_map(|field| field_document_symbol(view, field))
                .collect(),
        )
    }));

    symbols.extend(schema.types.iter().filter_map(|ty| {
        document_symbol_with_children(
            view,
            ty.name.clone(),
            SymbolKind::CLASS,
            Some("type".to_owned()),
            ty.span,
            ty.name_span,
            ty.fields
                .iter()
                .filter_map(|field| field_document_symbol(view, field))
                .collect(),
        )
    }));

    symbols.extend(schema.procedures.iter().filter_map(|procedure| {
        let detail = match procedure.kind {
            ProcedureKind::Query => "procedure".to_owned(),
            ProcedureKind::Mutation => "mutation procedure".to_owned(),
        };
        let mut children = procedure
            .args
            .iter()
            .filter_map(|arg| {
                document_symbol_leaf(
                    view,
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
            view,
            procedure.name.clone(),
            SymbolKind::FUNCTION,
            Some(detail),
            procedure.span,
            procedure.name_span,
            children,
        )
    }));

    // `query` blocks (cratestack#867) outline like procedures: a FUNCTION
    // with its parameters as children. The result type goes in `detail`
    // rather than becoming a child, matching how a procedure's return type
    // is rendered — the outline lists what a declaration *contains*, and a
    // query contains its parameters, not its result.
    symbols.extend(schema.queries.iter().filter_map(|query| {
        let mut children = query
            .args
            .iter()
            .filter_map(|arg| {
                document_symbol_leaf(
                    view,
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
            view,
            query.name.clone(),
            SymbolKind::FUNCTION,
            Some(format!("query -> {}", render_type_ref(&query.result_type))),
            query.span,
            query.name_span,
            children,
        )
    }));

    symbols
}
