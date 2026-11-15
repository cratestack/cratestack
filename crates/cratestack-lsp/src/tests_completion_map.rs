//! Completion of `@@map` (cratestack#1375).

use tower_lsp_server::ls_types::CompletionItemKind;

use crate::completion::completion_items;

/// cratestack#1375: `@@map("...")` is offered, saying it moves SQL only.
#[test]
fn map_attribute_is_offered_and_says_it_is_storage_only() {
    let items = completion_items(None, None);
    let map = items
        .iter()
        .find(|item| item.label == "@@map")
        .unwrap_or_else(|| panic!("completion list must offer @@map: {items:?}"));
    assert_eq!(map.kind, Some(CompletionItemKind::KEYWORD));
    let detail = map.detail.as_deref().expect("a detail string");
    assert!(
        detail.contains("routes") && detail.contains("unchanged"),
        "{detail}"
    );
}
