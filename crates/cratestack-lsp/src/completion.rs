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
        // cratestack#867 — a declarative custom-SQL read. Offered
        // alongside `procedure` because that is the construct authors
        // compare it against; `@@sql` is the attribute it carries.
        "query",
        "@@sql",
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

    // Multi-file keywords: reserved for the multi-file schema grammar
    // (cratestack#922, epic #910). Sourced from the parser rather than
    // copied, under the same "one list, N readers" rule as `builtin_types`
    // above. The detail string carries the caveat a bare keyword cannot: the
    // words are permanently unavailable as names, and the declarations they
    // introduce are not implemented yet, so inserting one today is always a
    // parse error.
    // Worded without naming either word, so it stays true for whatever the
    // parser's list holds.
    const MULTI_FILE_KEYWORD_DETAIL: &str = "reserved for multi-file schemas (cratestack#910) — never valid \
         as a name, and the declaration is not implemented yet";

    let mut items = keywords
        .into_iter()
        .map(|label| CompletionItem {
            label: label.to_owned(),
            kind: Some(CompletionItemKind::KEYWORD),
            ..CompletionItem::default()
        })
        .collect::<Vec<_>>();

    items.extend(
        cratestack_parser::reserved_multi_file_keywords()
            .iter()
            .copied()
            .map(|label| CompletionItem {
                label: label.to_owned(),
                kind: Some(CompletionItemKind::KEYWORD),
                detail: Some(MULTI_FILE_KEYWORD_DETAIL.to_owned()),
                ..CompletionItem::default()
            }),
    );

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

    items.extend(crate::mcp_completion::completion_items());

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

        // `query` blocks (cratestack#867). Completed the same way
        // procedures are — a query is not wire surface, but it *is*
        // callable Rust the schema author reads and writes here, so
        // leaving it out of completion would make the construct
        // second-class in the editor for no reason.
        for query in &schema.queries {
            if seen.insert(query.name.clone()) {
                items.push(CompletionItem {
                    label: query.name.clone(),
                    kind: Some(CompletionItemKind::FUNCTION),
                    detail: Some("query".to_owned()),
                    documentation: (!query.docs.is_empty()).then(|| {
                        Documentation::MarkupContent(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: query.docs.join("\n"),
                        })
                    }),
                    ..CompletionItem::default()
                });
            }
            for arg in &query.args {
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
