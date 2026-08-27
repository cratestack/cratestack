use crate::state::SymbolInfo;

/// The markdown body shown in a hover popup: a bolded kind and name, then the
/// rendered signature, then any `///` docs.
pub(crate) fn hover_markdown(symbol: &SymbolInfo) -> String {
    let mut value = format!("**{}** `{}`", symbol.kind, symbol.name);
    if !symbol.detail.is_empty() {
        value.push_str(&format!("\n\n`{}`", symbol.detail));
    }
    if !symbol.docs.is_empty() {
        value.push_str("\n\n");
        value.push_str(&symbol.docs.join("\n"));
    }
    value
}
