use tower_lsp_server::ls_types::{
    DocumentHighlight, DocumentHighlightKind, Location, Position, Range, Uri,
};

use crate::references::reference_spans_at;
use crate::state::DocumentState;
use crate::text::{position_to_offset, range_from_offsets};

/// Ranges mentioning the symbol under `position`, in file order.
///
/// Shared by `textDocument/references` and `textDocument/documentHighlight` so
/// the two can never disagree about what counts as a mention, and so neither
/// handler has to repeat the document/schema/offset guard chain.
pub(crate) fn reference_ranges(
    document: &DocumentState,
    position: Position,
    include_declaration: bool,
) -> Option<Vec<Range>> {
    let (text, schema) = document.resolved()?;
    let offset = position_to_offset(text, position)?;
    let spans = reference_spans_at(text, schema, offset, include_declaration)?;
    Some(
        spans
            .into_iter()
            .map(|span| range_from_offsets(text, span.start, span.end))
            .collect(),
    )
}

/// `textDocument/references`: every mention, as locations in `uri`.
pub(crate) fn reference_locations(
    uri: &Uri,
    document: &DocumentState,
    position: Position,
    include_declaration: bool,
) -> Option<Vec<Location>> {
    reference_ranges(document, position, include_declaration).map(|ranges| {
        ranges
            .into_iter()
            .map(|range| Location {
                uri: uri.clone(),
                range,
            })
            .collect()
    })
}

/// `textDocument/documentHighlight`: every mention, declaration included.
pub(crate) fn highlights(
    document: &DocumentState,
    position: Position,
) -> Option<Vec<DocumentHighlight>> {
    reference_ranges(document, position, true).map(|ranges| {
        ranges
            .into_iter()
            .map(|range| DocumentHighlight {
                range,
                kind: Some(DocumentHighlightKind::TEXT),
            })
            .collect()
    })
}
