use crate::state::SymbolInfo;

/// The markdown body shown in a hover popup: a bolded kind and name, then the
/// rendered signature, then any `///` docs.
///
/// `stale` marks a hover served from the last version of the file that parsed,
/// which happens whenever the current text has a syntax error. Saying so
/// matters: the alternative is silently describing a symbol as it was some
/// keystrokes ago, and leaving the reader to wonder why it disagrees with what
/// is on screen.
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
