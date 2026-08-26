use std::collections::BTreeSet;

use cratestack_core::{ProcedureKind, Schema};
use tower_lsp_server::ls_types::{
    CompletionItem, CompletionItemKind, Documentation, MarkupContent, MarkupKind,
};

use crate::type_ref::render_type_ref;

pub(crate) fn completion_items(schema: Option<&Schema>) -> Vec<CompletionItem> {
    let keywords = [
        "datasource",
        "auth",
        "mixin",
        "model",
        "type",
        "procedure",
        "mutation procedure",
        "mcp",
        "@use",
        "@id",
        "@unique",
        "@default",
        "@relation",
        "@allow",
        // `@custom` was removed in favor of `@computed`
        // (`docs/design/computed-fields.md`) — one concept, resolver-
        // backed response-time fields. `@computed(params: <Type>?)` is
        // the parameterized form; this flat keyword list has no snippet-
        // completion mechanism (no other entry here carries an
        // insert-text placeholder either), so only the bare marker is
        // offered.
        "@computed",
        "@@allow",
        "@@id",
        "@@unique",
    ];
    // Keywords that carry a hover/detail string, kept separate from the
    // bare `keywords` list above rather than converting every entry to a
    // tuple — `@@internal(...)` (cratestack#743,
    // `docs/design/route-suppression.md`) is easy to reach for and
    // mistake for a policy attribute (it looks like `@@allow`/`@@deny`
    // but isn't a policy expression at all), so it's the one keyword
    // where a one-line reminder in the completion popup earns its keep.
    let keywords_with_detail = [(
        "@@internal",
        "declares a model action unreachable from the wire (no REST route, RPC dispatch arm, or \
         client stub) — e.g. @@internal(\"create\"); purely a generation-time gate, not a policy \
         (see docs/design/route-suppression.md)",
    )];
    // Sourced from the parser's authoritative list rather than hand-copied,
    // so this can't silently drift the way it did before `Decimal` was
    // added here (cratestack#232) — a real editor regression that shipped
    // with nothing to catch it. `Page` is excluded: it's only valid as a
    // procedure return type (`Page<T>`), never a plain completable field
    // type — see `cratestack_parser::validate::type_names::validate_type_ref`.
    let builtin_types = cratestack_parser::builtin_type_names()
        .iter()
        .copied()
        .filter(|name| *name != "Page");

    // The three values `validate_datasource` (cratestack-parser) accepts for
    // `datasource { provider = "..." }`. `"none"` (cratestack#327) declares
    // a no-database, procedures-only schema — surfaced here so schema
    // authors discover it without reading source.
    let datasource_providers = [
        ("\"postgresql\"", "sqlx Postgres backend"),
        ("\"sqlite\"", "rusqlite embedded backend"),
        (
            "\"none\"",
            "no database (procedures-only server mode, cratestack#327) — no `model` block allowed",
        ),
    ];

    let mut items = keywords
        .into_iter()
        .map(|label| CompletionItem {
            label: label.to_owned(),
            kind: Some(CompletionItemKind::KEYWORD),
            ..CompletionItem::default()
        })
        .collect::<Vec<_>>();

    items.extend(
        keywords_with_detail
            .into_iter()
            .map(|(label, detail)| CompletionItem {
                label: label.to_owned(),
                kind: Some(CompletionItemKind::KEYWORD),
                detail: Some(detail.to_owned()),
                ..CompletionItem::default()
            }),
    );

    items.extend(builtin_types.into_iter().map(|label| CompletionItem {
        label: label.to_owned(),
        kind: Some(CompletionItemKind::TYPE_PARAMETER),
        ..CompletionItem::default()
    }));

    items.extend(
        datasource_providers
            .into_iter()
            .map(|(label, detail)| CompletionItem {
                label: label.to_owned(),
                kind: Some(CompletionItemKind::ENUM_MEMBER),
                detail: Some(detail.to_owned()),
                ..CompletionItem::default()
            }),
    );

    let mut seen = BTreeSet::new();
    if let Some(schema) = schema {
        for mixin in &schema.mixins {
            if seen.insert(mixin.name.clone()) {
                items.push(CompletionItem {
                    label: mixin.name.clone(),
                    kind: Some(CompletionItemKind::CLASS),
                    detail: Some("schema mixin".to_owned()),
                    documentation: (!mixin.docs.is_empty()).then(|| {
                        Documentation::MarkupContent(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: mixin.docs.join("\n"),
                        })
                    }),
                    ..CompletionItem::default()
                });
            }
        }
        for model in &schema.models {
            if seen.insert(model.name.clone()) {
                items.push(CompletionItem {
                    label: model.name.clone(),
                    kind: Some(CompletionItemKind::STRUCT),
                    detail: Some("schema model".to_owned()),
                    documentation: (!model.docs.is_empty()).then(|| {
                        Documentation::MarkupContent(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: model.docs.join("\n"),
                        })
                    }),
                    ..CompletionItem::default()
                });
            }
            for field in &model.fields {
                let detail = render_type_ref(&field.ty);
                if seen.insert(field.name.clone()) {
                    items.push(CompletionItem {
                        label: field.name.clone(),
                        kind: Some(CompletionItemKind::FIELD),
                        detail: Some(detail),
                        documentation: (!field.docs.is_empty()).then(|| {
                            Documentation::MarkupContent(MarkupContent {
                                kind: MarkupKind::Markdown,
                                value: field.docs.join("\n"),
                            })
                        }),
                        ..CompletionItem::default()
                    });
                }
            }
        }

        for ty in &schema.types {
            if seen.insert(ty.name.clone()) {
                items.push(CompletionItem {
                    label: ty.name.clone(),
                    kind: Some(CompletionItemKind::CLASS),
                    detail: Some("schema type".to_owned()),
                    documentation: (!ty.docs.is_empty()).then(|| {
                        Documentation::MarkupContent(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: ty.docs.join("\n"),
                        })
                    }),
                    ..CompletionItem::default()
                });
            }
        }

        for procedure in &schema.procedures {
            if seen.insert(procedure.name.clone()) {
                items.push(CompletionItem {
                    label: procedure.name.clone(),
                    kind: Some(CompletionItemKind::FUNCTION),
                    detail: Some(match procedure.kind {
                        ProcedureKind::Query => "procedure".to_owned(),
                        ProcedureKind::Mutation => "mutation procedure".to_owned(),
                    }),
                    documentation: (!procedure.docs.is_empty()).then(|| {
                        Documentation::MarkupContent(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: procedure.docs.join("\n"),
                        })
                    }),
                    ..CompletionItem::default()
                });
            }
            for arg in &procedure.args {
                if seen.insert(arg.name.clone()) {
                    items.push(CompletionItem {
                        label: arg.name.clone(),
                        kind: Some(CompletionItemKind::VARIABLE),
                        detail: Some(render_type_ref(&arg.ty)),
                        documentation: (!arg.docs.is_empty()).then(|| {
                            Documentation::MarkupContent(MarkupContent {
                                kind: MarkupKind::Markdown,
                                value: arg.docs.join("\n"),
                            })
                        }),
                        ..CompletionItem::default()
                    });
                }
            }
        }
    }

    items
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
