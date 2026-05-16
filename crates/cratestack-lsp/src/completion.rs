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
        "@custom",
        "@@allow",
    ];
    let builtin_types = [
        "String", "Cuid", "Int", "Float", "Boolean", "DateTime", "Json", "Bytes", "Uuid",
    ];

    let mut items = keywords
        .into_iter()
        .map(|label| CompletionItem {
            label: label.to_owned(),
            kind: Some(CompletionItemKind::KEYWORD),
            ..CompletionItem::default()
        })
        .collect::<Vec<_>>();

    items.extend(builtin_types.into_iter().map(|label| CompletionItem {
        label: label.to_owned(),
        kind: Some(CompletionItemKind::TYPE_PARAMETER),
        ..CompletionItem::default()
    }));

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
