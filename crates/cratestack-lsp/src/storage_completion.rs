//! Completion entry for `@@map` (cratestack#1375). Kept out of `completion.rs`,
//! which is already past the 200-line ceiling.

use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind};

pub(crate) fn completion_items() -> Vec<CompletionItem> {
    vec![CompletionItem {
        label: "@@map".to_owned(),
        kind: Some(CompletionItemKind::KEYWORD),
        detail: Some(
            "SQL table name only — routes, op ids and clients are unchanged; e.g. \
             @@map(\"legacy_accounts\") (cratestack#1375)"
                .to_owned(),
        ),
        ..CompletionItem::default()
    }]
}
