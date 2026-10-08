//! Completion coverage. The list is context-free, so these tests pin what it
//! offers and, where a list is sourced from the parser, that the two cannot
//! drift apart.

use tower_lsp_server::ls_types::CompletionItemKind;

use crate::completion::completion_items;

/// Regression test for cratestack#232: the builtin-type completion list
/// had silently drifted from `cratestack_parser::builtin_type_names()`
/// (missing `Decimal`), and nothing caught it. This pins the two lists
/// together so a future drift fails the suite instead of shipping.
#[test]
fn builtin_type_completions_match_parser_list_minus_page() {
    let labels: std::collections::BTreeSet<String> = completion_items(None, None)
        .into_iter()
        .filter(|item| item.kind == Some(CompletionItemKind::TYPE_PARAMETER))
        .map(|item| item.label)
        .collect();

    let expected: std::collections::BTreeSet<String> = cratestack_parser::builtin_type_names()
        .iter()
        .copied()
        .filter(|name| *name != "Page")
        .map(str::to_owned)
        .collect();

    assert_eq!(
        labels, expected,
        "completion list must track cratestack_parser::builtin_type_names() \
         (minus `Page`) — see cratestack#232",
    );
}

/// The multi-file grammar words (`part`/`import`, cratestack#922) must be
/// offered among the KEYWORD completions, sourced from the parser's
/// `reserved_multi_file_keywords()` so the two can't drift (the same
/// "one list, N readers" rule as cratestack#232's type list). Inclusion,
/// not equality, is the contract: `model`/`type`/`procedure` etc. are
/// keyword completions that are not (and must not be) part of the
/// reserved set.
#[test]
fn multi_file_keywords_are_offered_as_completions_with_reserved_detail() {
    let labels: std::collections::BTreeSet<String> = completion_items(None, None)
        .into_iter()
        .filter(|item| item.kind == Some(CompletionItemKind::KEYWORD))
        .map(|item| item.label)
        .collect();

    for keyword in cratestack_parser::reserved_multi_file_keywords() {
        assert!(
            labels.contains(*keyword),
            "completion list must offer the parser-reserved multi-file keyword \
             `{keyword}`: {labels:?}",
        );
    }

    for item in completion_items(None, None) {
        if cratestack_parser::reserved_multi_file_keywords().contains(&item.label.as_str()) {
            let detail = item.detail.as_deref().unwrap_or_default();
            assert!(
                detail.contains("reserved"),
                "multi-file keyword `{}` should carry a detail that says it is reserved, \
                 so completion doesn't imply a usable construct: {detail:?}",
                item.label,
            );
        }
    }
}

/// `@custom` was removed in favor of `@computed`
/// (`docs/design/computed-fields.md`) — the completion list must
/// offer the new attribute and never suggest the removed one, which
/// is now a parse error everywhere it's spelled.
#[test]
fn computed_attribute_is_offered_and_custom_is_gone() {
    let labels: std::collections::BTreeSet<String> = completion_items(None, None)
        .into_iter()
        .filter(|item| item.kind == Some(CompletionItemKind::KEYWORD))
        .map(|item| item.label)
        .collect();

    assert!(
        labels.contains("@computed"),
        "completion list must offer @computed: {labels:?}"
    );
    assert!(
        !labels.contains("@custom"),
        "completion list must never suggest the removed @custom attribute: {labels:?}"
    );
}

/// cratestack#743: `@@internal(...)` must be offered as a completion,
/// with a detail string distinguishing it from a policy attribute
/// (`@@allow`/`@@deny`) — it's easy to reach for by analogy and get
/// wrong, since it looks like one but isn't.
#[test]
fn internal_attribute_is_offered_with_a_detail_string() {
    let items = completion_items(None, None);
    let internal = items
        .iter()
        .find(|item| item.label == "@@internal")
        .unwrap_or_else(|| panic!("completion list must offer @@internal: {items:?}"));
    assert_eq!(internal.kind, Some(CompletionItemKind::KEYWORD));
    let detail = internal
        .detail
        .as_deref()
        .unwrap_or_else(|| panic!("@@internal completion should carry a detail string"));
    assert!(
        detail.contains("generation-time"),
        "@@internal's detail should distinguish it from a policy attribute like @@allow: \
         {detail}"
    );
}

/// cratestack#327: `datasource { provider = "none" }` must be offered
/// alongside the existing `"postgresql"`/`"sqlite"` provider values.
#[test]
fn datasource_provider_completions_include_none_alongside_postgresql_and_sqlite() {
    let labels: std::collections::BTreeSet<String> = completion_items(None, None)
        .into_iter()
        .filter(|item| item.kind == Some(CompletionItemKind::ENUM_MEMBER))
        .map(|item| item.label)
        .collect();

    assert_eq!(
        labels,
        std::collections::BTreeSet::from([
            "\"postgresql\"".to_owned(),
            "\"sqlite\"".to_owned(),
            "\"none\"".to_owned(),
        ])
    );
}

/// ADR 0019 D5: the field attributes offered inside a block are exactly the
/// list the parser checks that block against (`cratestack_parser::
/// field_attribute_names`), so completion and validation cannot drift. The
/// check is run through the cursor placement the server uses.
#[test]
fn field_attribute_completions_are_exactly_the_blocks_list() {
    use cratestack_parser::{FieldHost, field_attribute_names};
    use std::collections::BTreeSet;

    let every_field_attribute: BTreeSet<&str> = FieldHost::ALL
        .into_iter()
        .flat_map(field_attribute_names)
        .collect();
    for (host, header) in [
        (FieldHost::Model, "model Note"),
        (FieldHost::View, "view Summary from Note"),
        (FieldHost::Mixin, "mixin Audited"),
        (FieldHost::Type, "type Args"),
        (FieldHost::Auth, "auth Caller"),
    ] {
        let text = format!("{header} {{\n  id Int \n}}\n");
        let offset = text.find("Int ").unwrap() + "Int ".len();
        let placed = crate::field_host::enclosing_field_host(&text, offset);
        assert_eq!(placed, Some(host), "{header}");

        let labels: BTreeSet<String> = completion_items(None, placed)
            .into_iter()
            .map(|item| item.label)
            .collect();
        let offered: BTreeSet<&str> = every_field_attribute
            .iter()
            .copied()
            .filter(|name| labels.contains(*name))
            .collect();
        assert_eq!(
            offered,
            field_attribute_names(host).into_iter().collect(),
            "{header}: the offered field attributes must be the block's list"
        );
        assert!(
            !labels.contains("@allow"),
            "{header} refuses a field `@allow`"
        );
    }
}

/// With no block to go by, every list's names are offered once, and the
/// procedure-position `@allow` too.
#[test]
fn outside_a_block_every_field_attribute_is_offered_once() {
    use cratestack_parser::{FieldHost, field_attribute_names};

    let labels: Vec<String> = completion_items(None, None)
        .into_iter()
        .map(|item| item.label)
        .collect();
    for name in FieldHost::ALL.into_iter().flat_map(field_attribute_names) {
        assert_eq!(
            labels.iter().filter(|label| label.as_str() == name).count(),
            1,
            "{name}"
        );
    }
    assert!(labels.iter().any(|label| label == "@allow"));
}
