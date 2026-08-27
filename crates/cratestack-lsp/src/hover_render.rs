use tower_lsp_server::ls_types::{Hover, HoverContents, MarkupContent, MarkupKind, Position};

use crate::hover::locate_symbol;
use crate::state::DocumentState;
use crate::text::{position_to_offset, span_to_range};

/// The hover response for a position, or `None` when there is nothing to show.
pub(crate) fn hover_at(document: &DocumentState, position: Position) -> Option<Hover> {
    let (text, schema) = document.resolved()?;
    let offset = position_to_offset(text, position)?;
    let symbol = locate_symbol(schema, offset)?;
    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: hover_markdown(&symbol, document.is_stale()),
        }),
        range: span_to_range(text, symbol.selection_span),
    })
}

/// The markdown body shown in a hover popup: a bolded kind and name, then the
/// rendered signature, then any `///` docs.
///
/// `stale` marks a hover served from the last version of the file that parsed,
/// which happens whenever the current text has a syntax error. Saying so
/// matters: the alternative is silently describing a symbol as it was some
/// keystrokes ago, and leaving the reader to wonder why it disagrees with what
/// is on screen.
use crate::state::SymbolInfo;

pub(crate) fn hover_markdown(symbol: &SymbolInfo, stale: bool) -> String {
    let mut value = format!("**{}** `{}`", symbol.kind, symbol.name);
    if !symbol.detail.is_empty() {
        value.push_str(&format!("\n\n`{}`", symbol.detail));
    }
    if !symbol.docs.is_empty() {
        value.push_str("\n\n");
        value.push_str(&symbol.docs.join("\n"));
    }
    if stale {
        value.push_str("\n\n---\n\n*From the last version of this file that parsed.*");
    }
    value
}
