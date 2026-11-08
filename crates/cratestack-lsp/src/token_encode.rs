//! Delta encoding of semantic tokens.

use cratestack_core::SourceSpan;
use tower_lsp_server::ls_types::SemanticToken;

use crate::text::offset_to_position;
use crate::view::Resolved;

/// LSP wants tokens sorted and encoded relative to their predecessor, with
/// lengths and columns in UTF-16 code units.
pub(crate) fn delta_encode(
    view: &Resolved<'_>,
    entries: Vec<(SourceSpan, u32)>,
) -> Vec<SemanticToken> {
    let text = view.text;
    let mut entries: Vec<_> = entries
        .into_iter()
        .filter_map(|(span, kind)| Some((view.local_span(span)?, kind)))
        .collect();
    entries.sort_by_key(|(span, _)| (span.start, span.end));
    entries.dedup_by_key(|(span, _)| (span.start, span.end));

    let mut tokens = Vec::new();
    let mut previous_line = 0u32;
    let mut previous_start = 0u32;

    for (span, token_type) in entries {
        let start = offset_to_position(text, span.start);
        let end = offset_to_position(text, span.end);
        // A multi-line token is not expressible in this encoding; the protocol
        // has no length that spans a newline.
        if end.line != start.line || end.character <= start.character {
            continue;
        }
        let delta_line = start.line - previous_line;
        let delta_start = if delta_line == 0 {
            start.character - previous_start
        } else {
            start.character
        };
        tokens.push(SemanticToken {
            delta_line,
            delta_start,
            length: end.character - start.character,
            token_type,
            token_modifiers_bitset: 0,
        });
        previous_line = start.line;
        previous_start = start.character;
    }

    tokens
}
