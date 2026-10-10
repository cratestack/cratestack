//! Field-attribute completions, read from the parser's closed lists
//! (`cratestack_parser::field_attribute_names`, ADR 0019 D5) so completion
//! and validation cannot drift. It used to be a hand-written list of five
//! names, missing most of the language and offering `@allow`, which a field
//! refuses.
//!
//! Inside a `model`, `view`, `mixin`, `type` or `auth` block the items are
//! exactly that block's list; anywhere else (the top level, a procedure's
//! attributes, a document the cursor could not be placed in) they are the
//! names of every list, once each.
//!
//! The lists give no snippet: this server's completions are plain labels,
//! so an attribute that takes an argument list (`@default`, `@length`) is
//! offered bare, and `@computed(params: <Type>?)` as its bare marker.

use cratestack_parser::{FieldHost, field_attribute_names};
use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind};

/// The field attributes to offer for a cursor in `host`.
pub(crate) fn field_attribute_items(host: Option<FieldHost>) -> Vec<CompletionItem> {
    let mut names: Vec<&'static str> = match host {
        Some(host) => field_attribute_names(host),
        None => FieldHost::ALL
            .into_iter()
            .flat_map(field_attribute_names)
            .collect(),
    };
    let mut seen = std::collections::BTreeSet::new();
    names.retain(|name| seen.insert(*name));
    names
        .into_iter()
        .map(|label| CompletionItem {
            label: label.to_owned(),
            kind: Some(CompletionItemKind::KEYWORD),
            ..CompletionItem::default()
        })
        .collect()
}
