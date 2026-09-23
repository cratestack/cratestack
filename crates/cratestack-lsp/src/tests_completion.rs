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
    let labels: std::collections::BTreeSet<String> = completion_items(None)
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
    let labels: std::collections::BTreeSet<String> = completion_items(None)
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

    for item in completion_items(None) {
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
    let labels: std::collections::BTreeSet<String> = completion_items(None)
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
    let items = completion_items(None);
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
    let labels: std::collections::BTreeSet<String> = completion_items(None)
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
