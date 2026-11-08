//! Builders of the outline's `DocumentSymbol`s. A symbol whose span is not in
//! the requesting file (a declaration of another file of a split schema, or a
//! mixin field copied into a model from elsewhere) is not built.

use tower_lsp_server::ls_types::{DocumentSymbol, SymbolKind};

use cratestack_core::SourceSpan;

use crate::type_ref::render_type_ref;
use crate::view::Resolved;

pub(crate) fn field_document_symbol(
    view: &Resolved<'_>,
    field: &cratestack_core::Field,
) -> Option<DocumentSymbol> {
    document_symbol_leaf(
        view,
        field.name.clone(),
        SymbolKind::FIELD,
        Some(render_type_ref(&field.ty)),
        field.span,
        field.name_span,
    )
}

#[allow(deprecated)]
pub(crate) fn document_symbol_with_children(
    view: &Resolved<'_>,
    name: String,
    kind: SymbolKind,
    detail: Option<String>,
    span: SourceSpan,
    selection_span: SourceSpan,
    children: Vec<DocumentSymbol>,
) -> Option<DocumentSymbol> {
    Some(DocumentSymbol {
        name,
        detail,
        kind,
        tags: None,
        deprecated: None,
        range: view.range(span)?,
        selection_range: view.range(selection_span)?,
        children: Some(children),
    })
}

#[allow(deprecated)]
pub(crate) fn document_symbol_leaf(
    view: &Resolved<'_>,
    name: String,
    kind: SymbolKind,
    detail: Option<String>,
    span: SourceSpan,
    selection_span: SourceSpan,
) -> Option<DocumentSymbol> {
    Some(DocumentSymbol {
        name,
        detail,
        kind,
        tags: None,
        deprecated: None,
        range: view.range(span)?,
        selection_range: view.range(selection_span)?,
        children: None,
    })
}
