//! The completion entry for `@authn` (ADR 0022, cratestack#1244), with its
//! argument shapes in the detail: they are what the parser refuses.

use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind};

const DETAIL: &str = "procedure attribute: how recently and how strongly the caller must have \
     authenticated — `@authn(maxAge: 300, amr: [\"hwk\"], acr: [\"aal2\"])`; any of `maxAge` \
     (seconds, 1 to one year), `amr` (methods that must all be present) and `acr` (acceptable \
     classes). Checked after the allow and deny policy; unmet is a 401 step-up (RFC 9470)";

pub(crate) fn completion_items() -> impl Iterator<Item = CompletionItem> {
    std::iter::once(CompletionItem {
        label: cratestack_core::schema::AUTHN_ATTRIBUTE.to_owned(),
        kind: Some(CompletionItemKind::KEYWORD),
        detail: Some(DETAIL.to_owned()),
        ..CompletionItem::default()
    })
}
