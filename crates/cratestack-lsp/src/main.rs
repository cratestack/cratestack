use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use cratestack_core::{Attribute, ProcedureKind, Schema, SourceSpan, TypeRef};
use tokio::sync::RwLock;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, CompletionOptions, CompletionParams, CompletionResponse,
    Diagnostic, DiagnosticSeverity, DidChangeTextDocumentParams, DidOpenTextDocumentParams,
    DocumentSymbol, DocumentSymbolParams, DocumentSymbolResponse, GotoDefinitionParams,
    GotoDefinitionResponse, Hover, HoverContents, HoverParams, HoverProviderCapability,
    InitializeParams, InitializeResult, InitializedParams, Location, MarkupContent, MarkupKind,
    MessageType, OneOf, Position, Range, ServerCapabilities, ServerInfo, SymbolKind,
    TextDocumentSyncCapability, TextDocumentSyncKind, Url,
};
use tower_lsp::{Client, LanguageServer, LspService, Server};

#[derive(Clone)]
struct DocumentState {
    text: String,
    schema: Option<Schema>,
}

#[derive(Clone)]
struct SymbolInfo {
    kind: &'static str,
    name: String,
    detail: String,
    docs: Vec<String>,
    selection_span: SourceSpan,
}

#[derive(Clone)]
struct SpannedName {
    name: String,
    span: SourceSpan,
}

struct ParsedRelationAttributeSpans {
    fields: Vec<SpannedName>,
    references: Vec<SpannedName>,
}

struct Backend {
    client: Client,
    documents: Arc<RwLock<HashMap<Url, DocumentState>>>,
}

impl Backend {
    fn new(client: Client) -> Self {
        Self {
            client,
            documents: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    async fn update_document(&self, uri: Url, text: String) {
        let (schema, diagnostics) = analyze_document(&uri, &text);
        self.documents
            .write()
            .await
            .insert(uri.clone(), DocumentState { text, schema });
        self.client
            .publish_diagnostics(uri, diagnostics, None)
            .await;
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            server_info: Some(ServerInfo {
                name: "cratestack-lsp".to_owned(),
                version: Some(env!("CARGO_PKG_VERSION").to_owned()),
            }),
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                completion_provider: Some(CompletionOptions::default()),
                definition_provider: Some(OneOf::Left(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
                ..ServerCapabilities::default()
            },
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "cratestack-lsp initialized")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.update_document(params.text_document.uri, params.text_document.text)
            .await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        if let Some(change) = params.content_changes.into_iter().last() {
            self.update_document(params.text_document.uri, change.text)
                .await;
        }
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let text_document_position = params.text_document_position_params;
        let documents = self.documents.read().await;
        let Some(document) = documents.get(&text_document_position.text_document.uri) else {
            return Ok(None);
        };
        let Some(schema) = &document.schema else {
            return Ok(None);
        };
        let Some(offset) = position_to_offset(&document.text, text_document_position.position)
        else {
            return Ok(None);
        };
        let Some(symbol) = locate_symbol(schema, offset) else {
            return Ok(None);
        };
        let range = span_to_range(&document.text, symbol.selection_span);
        let mut value = format!("**{}** `{}`", symbol.kind, symbol.name);
        if !symbol.detail.is_empty() {
            value.push_str("\n\n");
            value.push_str(&format!("`{}`", symbol.detail));
        }
        if !symbol.docs.is_empty() {
            value.push_str("\n\n");
            value.push_str(&symbol.docs.join("\n"));
        }

        Ok(Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value,
            }),
            range,
        }))
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let documents = self.documents.read().await;
        let schema = documents
            .get(&params.text_document_position.text_document.uri)
            .and_then(|document| document.schema.as_ref());
        Ok(Some(CompletionResponse::Array(completion_items(schema))))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let text_document_position = params.text_document_position_params;
        let documents = self.documents.read().await;
        let Some(document) = documents.get(&text_document_position.text_document.uri) else {
            return Ok(None);
        };
        let Some(schema) = &document.schema else {
            return Ok(None);
        };
        let Some(offset) = position_to_offset(&document.text, text_document_position.position)
        else {
            return Ok(None);
        };
        let Some(location) = definition_location(
            &text_document_position.text_document.uri,
            &document.text,
            schema,
            offset,
        ) else {
            return Ok(None);
        };
        Ok(Some(GotoDefinitionResponse::Scalar(location)))
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let documents = self.documents.read().await;
        let Some(document) = documents.get(&params.text_document.uri) else {
            return Ok(None);
        };
        let Some(schema) = &document.schema else {
            return Ok(None);
        };
        Ok(Some(DocumentSymbolResponse::Nested(document_symbols(
            &document.text,
            schema,
        ))))
    }
}

fn analyze_document(uri: &Url, text: &str) -> (Option<Schema>, Vec<Diagnostic>) {
    let label = uri
        .to_file_path()
        .ok()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| uri.to_string());

    match cratestack_parser::parse_schema_named(&label, text) {
        Ok(schema) => (Some(schema), Vec::new()),
        Err(error) => (None, vec![schema_error_to_diagnostic(text, &error)]),
    }
}

fn schema_error_to_diagnostic(text: &str, error: &cratestack_parser::SchemaError) -> Diagnostic {
    let span = precise_relation_error_span(text, error).unwrap_or_else(|| error.span());
    Diagnostic {
        range: range_from_offsets(text, span.start, span.end),
        severity: Some(DiagnosticSeverity::ERROR),
        message: error.message().to_owned(),
        source: Some("cratestack".to_owned()),
        ..Diagnostic::default()
    }
}

fn precise_relation_error_span(
    text: &str,
    error: &cratestack_parser::SchemaError,
) -> Option<std::ops::Range<usize>> {
    let (field_name, list_key) = if let Some(name) =
        extract_message_field_name(error.message(), "unknown local field `")
    {
        (name, "fields")
    } else if let Some(name) = extract_message_field_name(error.message(), "unknown target field `")
    {
        (name, "references")
    } else {
        return None;
    };

    let line_text = text.lines().nth(error.line().saturating_sub(1))?;
    let line_start = *line_start_offsets(text).get(error.line().saturating_sub(1))?;
    let attribute = relation_attribute_from_line(line_text, line_start, error.line())?;
    let relation = parse_relation_attribute_spans(&attribute)?;
    let target = match list_key {
        "fields" => relation
            .fields
            .into_iter()
            .find(|name| name.name == field_name)?,
        "references" => relation
            .references
            .into_iter()
            .find(|name| name.name == field_name)?,
        _ => return None,
    };

    Some(target.span.start..target.span.end)
}

fn extract_message_field_name<'a>(message: &'a str, prefix: &str) -> Option<&'a str> {
    let suffix = message.split_once(prefix)?.1;
    suffix.split('`').next()
}

fn relation_attribute_from_line(
    line_text: &str,
    line_start: usize,
    line_number: usize,
) -> Option<Attribute> {
    let raw_start = line_text.find("@relation(")?;
    let raw = line_text[raw_start..].trim_end().to_owned();
    Some(Attribute {
        raw: raw.clone(),
        span: SourceSpan {
            start: line_start + raw_start,
            end: line_start + raw_start + raw.len(),
            line: line_number,
        },
    })
}

fn completion_items(schema: Option<&Schema>) -> Vec<CompletionItem> {
    let keywords = [
        "datasource",
        "auth",
        "model",
        "type",
        "procedure",
        "mutation procedure",
        "mcp",
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
        for model in &schema.models {
            if seen.insert(model.name.clone()) {
                items.push(CompletionItem {
                    label: model.name.clone(),
                    kind: Some(CompletionItemKind::STRUCT),
                    detail: Some("schema model".to_owned()),
                    documentation: (!model.docs.is_empty()).then(|| {
                        tower_lsp::lsp_types::Documentation::MarkupContent(MarkupContent {
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
                            tower_lsp::lsp_types::Documentation::MarkupContent(MarkupContent {
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
                        tower_lsp::lsp_types::Documentation::MarkupContent(MarkupContent {
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
                        tower_lsp::lsp_types::Documentation::MarkupContent(MarkupContent {
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
                            tower_lsp::lsp_types::Documentation::MarkupContent(MarkupContent {
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

fn locate_symbol(schema: &Schema, offset: usize) -> Option<SymbolInfo> {
    if let Some(datasource) = &schema.datasource
        && span_contains(datasource.span, offset)
    {
        return Some(SymbolInfo {
            kind: "datasource",
            name: datasource.name.clone(),
            detail: "datasource block".to_owned(),
            docs: datasource.docs.clone(),
            selection_span: datasource.span,
        });
    }

    if let Some(auth) = &schema.auth {
        for field in &auth.fields {
            if span_contains(field.ty.name_span, offset)
                && let Some(symbol) = named_type_symbol(schema, &field.ty, offset)
            {
                return Some(symbol);
            }
            if span_contains(field.span, offset) {
                return Some(field_symbol(field));
            }
        }
        if span_contains(auth.span, offset) {
            return Some(SymbolInfo {
                kind: "auth",
                name: auth.name.clone(),
                detail: "auth block".to_owned(),
                docs: auth.docs.clone(),
                selection_span: auth.span,
            });
        }
    }

    for model in &schema.models {
        for field in &model.fields {
            if span_contains(field.ty.name_span, offset)
                && let Some(symbol) = named_type_symbol(schema, &field.ty, offset)
            {
                return Some(symbol);
            }
            if span_contains(field.span, offset) {
                return Some(field_symbol(field));
            }
        }
        if span_contains(model.name_span, offset) {
            return Some(SymbolInfo {
                kind: "model",
                name: model.name.clone(),
                detail: "model".to_owned(),
                docs: model.docs.clone(),
                selection_span: model.name_span,
            });
        }
    }

    for ty in &schema.types {
        for field in &ty.fields {
            if span_contains(field.ty.name_span, offset)
                && let Some(symbol) = named_type_symbol(schema, &field.ty, offset)
            {
                return Some(symbol);
            }
            if span_contains(field.span, offset) {
                return Some(field_symbol(field));
            }
        }
        if span_contains(ty.name_span, offset) {
            return Some(SymbolInfo {
                kind: "type",
                name: ty.name.clone(),
                detail: "type".to_owned(),
                docs: ty.docs.clone(),
                selection_span: ty.name_span,
            });
        }
    }

    for procedure in &schema.procedures {
        for arg in &procedure.args {
            if span_contains(arg.ty.name_span, offset)
                && let Some(symbol) = named_type_symbol(schema, &arg.ty, offset)
            {
                return Some(symbol);
            }
            if span_contains(arg.span, offset) {
                return Some(SymbolInfo {
                    kind: "argument",
                    name: arg.name.clone(),
                    detail: render_type_ref(&arg.ty),
                    docs: arg.docs.clone(),
                    selection_span: arg.name_span,
                });
            }
        }
        if type_ref_at_offset(&procedure.return_type, offset)
            && let Some(symbol) = named_type_symbol(schema, &procedure.return_type, offset)
        {
            return Some(symbol);
        }
        if span_contains(procedure.name_span, offset) {
            let kind = match procedure.kind {
                ProcedureKind::Query => "procedure",
                ProcedureKind::Mutation => "mutation procedure",
            };
            let detail = format!("{} -> {}", kind, render_type_ref(&procedure.return_type));
            return Some(SymbolInfo {
                kind,
                name: procedure.name.clone(),
                detail,
                docs: procedure.docs.clone(),
                selection_span: procedure.name_span,
            });
        }
    }

    None
}

fn definition_location(uri: &Url, text: &str, schema: &Schema, offset: usize) -> Option<Location> {
    let span = relation_target_span(schema, offset)
        .or_else(|| type_reference_target_span(schema, offset))
        .or_else(|| word_at_offset(text, offset).and_then(|word| declaration_span(schema, word)))?;
    Some(Location {
        uri: uri.clone(),
        range: range_from_offsets(text, span.start, span.end),
    })
}

fn declaration_span(schema: &Schema, word: &str) -> Option<SourceSpan> {
    if let Some(datasource) = &schema.datasource
        && datasource.name == word
    {
        return Some(datasource.span);
    }
    if let Some(auth) = &schema.auth {
        if auth.name == word {
            return Some(auth.span);
        }
        if let Some(field) = auth.fields.iter().find(|field| field.name == word) {
            return Some(field.name_span);
        }
    }
    for model in &schema.models {
        if model.name == word {
            return Some(model.name_span);
        }
        if let Some(field) = model.fields.iter().find(|field| field.name == word) {
            return Some(field.name_span);
        }
    }
    for ty in &schema.types {
        if ty.name == word {
            return Some(ty.name_span);
        }
        if let Some(field) = ty.fields.iter().find(|field| field.name == word) {
            return Some(field.name_span);
        }
    }
    for procedure in &schema.procedures {
        if procedure.name == word {
            return Some(procedure.name_span);
        }
        if let Some(arg) = procedure.args.iter().find(|arg| arg.name == word) {
            return Some(arg.name_span);
        }
    }
    None
}

fn type_reference_target_span(schema: &Schema, offset: usize) -> Option<SourceSpan> {
    for model in &schema.models {
        for field in &model.fields {
            if span_contains(field.ty.name_span, offset) {
                return declaration_span(schema, &field.ty.name);
            }
        }
    }
    for ty in &schema.types {
        for field in &ty.fields {
            if span_contains(field.ty.name_span, offset) {
                return declaration_span(schema, &field.ty.name);
            }
        }
    }
    for procedure in &schema.procedures {
        if let Some(target) = nested_type_reference_name_at_offset(&procedure.return_type, offset) {
            return declaration_span(schema, target);
        }
        for arg in &procedure.args {
            if let Some(target) = nested_type_reference_name_at_offset(&arg.ty, offset) {
                return declaration_span(schema, target);
            }
        }
    }
    None
}

fn relation_target_span(schema: &Schema, offset: usize) -> Option<SourceSpan> {
    for model in &schema.models {
        for field in &model.fields {
            let Some(relation) = relation_attribute_spans(&field.attributes) else {
                continue;
            };
            if let Some(name) = relation
                .fields
                .iter()
                .find(|name| span_contains(name.span, offset))
                && let Some(target) = model
                    .fields
                    .iter()
                    .find(|candidate| candidate.name == name.name)
            {
                return Some(target.name_span);
            }
            if let Some(name) = relation
                .references
                .iter()
                .find(|name| span_contains(name.span, offset))
            {
                let Some(related_model) = schema
                    .models
                    .iter()
                    .find(|candidate| candidate.name == field.ty.name)
                else {
                    continue;
                };
                if let Some(target) = related_model
                    .fields
                    .iter()
                    .find(|candidate| candidate.name == name.name)
                {
                    return Some(target.name_span);
                }
            }
        }
    }
    None
}

fn document_symbols(text: &str, schema: &Schema) -> Vec<DocumentSymbol> {
    let mut symbols = Vec::new();

    if let Some(datasource) = &schema.datasource {
        symbols.push(document_symbol_leaf(
            text,
            datasource.name.clone(),
            SymbolKind::OBJECT,
            Some("datasource".to_owned()),
            datasource.span,
            datasource.span,
        ));
    }

    if let Some(auth) = &schema.auth {
        symbols.push(document_symbol_with_children(
            text,
            auth.name.clone(),
            SymbolKind::OBJECT,
            Some("auth".to_owned()),
            auth.span,
            auth.span,
            auth.fields
                .iter()
                .map(|field| field_document_symbol(text, field))
                .collect(),
        ));
    }

    symbols.extend(schema.models.iter().map(|model| {
        document_symbol_with_children(
            text,
            model.name.clone(),
            SymbolKind::STRUCT,
            Some("model".to_owned()),
            model.span,
            model.name_span,
            model
                .fields
                .iter()
                .map(|field| field_document_symbol(text, field))
                .collect(),
        )
    }));

    symbols.extend(schema.types.iter().map(|ty| {
        document_symbol_with_children(
            text,
            ty.name.clone(),
            SymbolKind::CLASS,
            Some("type".to_owned()),
            ty.span,
            ty.name_span,
            ty.fields
                .iter()
                .map(|field| field_document_symbol(text, field))
                .collect(),
        )
    }));

    symbols.extend(schema.procedures.iter().map(|procedure| {
        let detail = match procedure.kind {
            ProcedureKind::Query => "procedure".to_owned(),
            ProcedureKind::Mutation => "mutation procedure".to_owned(),
        };
        let mut children = procedure
            .args
            .iter()
            .map(|arg| {
                document_symbol_leaf(
                    text,
                    arg.name.clone(),
                    SymbolKind::VARIABLE,
                    Some(render_type_ref(&arg.ty)),
                    arg.span,
                    arg.name_span,
                )
            })
            .collect::<Vec<_>>();
        children.sort_by_key(|symbol| (symbol.range.start.line, symbol.range.start.character));
        document_symbol_with_children(
            text,
            procedure.name.clone(),
            SymbolKind::FUNCTION,
            Some(detail),
            procedure.span,
            procedure.name_span,
            children,
        )
    }));

    symbols
}

fn field_document_symbol(text: &str, field: &cratestack_core::Field) -> DocumentSymbol {
    document_symbol_leaf(
        text,
        field.name.clone(),
        SymbolKind::FIELD,
        Some(render_type_ref(&field.ty)),
        field.span,
        field.name_span,
    )
}

#[allow(deprecated)]
fn document_symbol_with_children(
    text: &str,
    name: String,
    kind: SymbolKind,
    detail: Option<String>,
    span: SourceSpan,
    selection_span: SourceSpan,
    children: Vec<DocumentSymbol>,
) -> DocumentSymbol {
    let range = range_from_offsets(text, span.start, span.end);
    let selection_range = range_from_offsets(text, selection_span.start, selection_span.end);
    DocumentSymbol {
        name,
        detail,
        kind,
        tags: None,
        deprecated: None,
        range,
        selection_range,
        children: Some(children),
    }
}

#[allow(deprecated)]
fn document_symbol_leaf(
    text: &str,
    name: String,
    kind: SymbolKind,
    detail: Option<String>,
    span: SourceSpan,
    selection_span: SourceSpan,
) -> DocumentSymbol {
    let range = range_from_offsets(text, span.start, span.end);
    let selection_range = range_from_offsets(text, selection_span.start, selection_span.end);
    DocumentSymbol {
        name,
        detail,
        kind,
        tags: None,
        deprecated: None,
        range,
        selection_range,
        children: None,
    }
}

fn field_symbol(field: &cratestack_core::Field) -> SymbolInfo {
    SymbolInfo {
        kind: "field",
        name: field.name.clone(),
        detail: render_type_ref(&field.ty),
        docs: field.docs.clone(),
        selection_span: field.name_span,
    }
}

fn named_type_symbol(schema: &Schema, ty: &TypeRef, offset: usize) -> Option<SymbolInfo> {
    if let Some(inner) = ty
        .generic_args
        .iter()
        .find(|inner| type_ref_at_offset(inner, offset))
    {
        return named_type_symbol(schema, inner, offset);
    }
    schema
        .models
        .iter()
        .find(|model| model.name == ty.name)
        .map(|model| SymbolInfo {
            kind: "model",
            name: model.name.clone(),
            detail: "model".to_owned(),
            docs: model.docs.clone(),
            selection_span: model.name_span,
        })
        .or_else(|| {
            schema
                .types
                .iter()
                .find(|decl| decl.name == ty.name)
                .map(|decl| SymbolInfo {
                    kind: "type",
                    name: decl.name.clone(),
                    detail: "type".to_owned(),
                    docs: decl.docs.clone(),
                    selection_span: decl.name_span,
                })
        })
}

fn relation_attribute_spans(attributes: &[Attribute]) -> Option<ParsedRelationAttributeSpans> {
    let attribute = attributes
        .iter()
        .find(|attribute| attribute.raw.starts_with("@relation("))?;
    parse_relation_attribute_spans(attribute)
}

fn parse_relation_attribute_spans(attribute: &Attribute) -> Option<ParsedRelationAttributeSpans> {
    let raw = attribute.raw.trim();
    let inner = raw.strip_prefix("@relation(")?.strip_suffix(')')?;
    let inner_offset = attribute.raw.find('(')? + 1;
    let mut fields = None;
    let mut references = None;

    for (entry, start, _) in split_top_level_ranges(inner, ',', inner_offset) {
        let colon = entry.find(':')?;
        let key = entry[..colon].trim();
        let value = entry[colon + 1..].trim();
        let value_offset = start + colon + 1 + entry[colon + 1..].find(value).unwrap_or_default();
        match key {
            "fields" => {
                fields = Some(parse_relation_name_list(
                    value,
                    attribute.span.line,
                    attribute.span.start + value_offset,
                )?)
            }
            "references" => {
                references = Some(parse_relation_name_list(
                    value,
                    attribute.span.line,
                    attribute.span.start + value_offset,
                )?)
            }
            _ => {}
        }
    }

    Some(ParsedRelationAttributeSpans {
        fields: fields?,
        references: references?,
    })
}

fn parse_relation_name_list(
    value: &str,
    line: usize,
    absolute_start: usize,
) -> Option<Vec<SpannedName>> {
    let inner = value.strip_prefix('[')?.strip_suffix(']')?;
    let list_start = absolute_start + 1;
    let mut names = Vec::new();
    for (entry, start, end) in split_top_level_ranges(inner, ',', 0) {
        if entry.is_empty() {
            continue;
        }
        names.push(SpannedName {
            name: entry.to_owned(),
            span: SourceSpan {
                start: list_start + start,
                end: list_start + end,
                line,
            },
        });
    }
    Some(names)
}

fn split_top_level_ranges(
    input: &str,
    separator: char,
    offset: usize,
) -> Vec<(String, usize, usize)> {
    let mut entries = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (index, ch) in input.char_indices() {
        match ch {
            '[' | '(' => depth += 1,
            ']' | ')' => depth = depth.saturating_sub(1),
            ch if ch == separator && depth == 0 => {
                let raw = &input[start..index];
                let trimmed = raw.trim();
                if !trimmed.is_empty() {
                    let trim_start = raw.find(trimmed).unwrap_or_default();
                    entries.push((
                        trimmed.to_owned(),
                        offset + start + trim_start,
                        offset + start + trim_start + trimmed.len(),
                    ));
                }
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }
    let raw = &input[start..];
    let trimmed = raw.trim();
    if !trimmed.is_empty() {
        let trim_start = raw.find(trimmed).unwrap_or_default();
        entries.push((
            trimmed.to_owned(),
            offset + start + trim_start,
            offset + start + trim_start + trimmed.len(),
        ));
    }
    entries
}

fn render_type_ref(ty: &TypeRef) -> String {
    let base = if ty.generic_args.is_empty() {
        ty.name.clone()
    } else {
        format!(
            "{}<{}>",
            ty.name,
            ty.generic_args
                .iter()
                .map(render_type_ref)
                .collect::<Vec<_>>()
                .join(", ")
        )
    };

    match ty.arity {
        cratestack_core::TypeArity::Required => base,
        cratestack_core::TypeArity::Optional => format!("{base}?"),
        cratestack_core::TypeArity::List => format!("{base}[]"),
    }
}

fn type_ref_at_offset(ty: &TypeRef, offset: usize) -> bool {
    span_contains(ty.name_span, offset)
        || ty
            .generic_args
            .iter()
            .any(|inner| type_ref_at_offset(inner, offset))
}

fn nested_type_reference_name_at_offset(ty: &TypeRef, offset: usize) -> Option<&str> {
    if span_contains(ty.name_span, offset) {
        return Some(ty.name.as_str());
    }
    ty.generic_args
        .iter()
        .find_map(|inner| nested_type_reference_name_at_offset(inner, offset))
}

fn span_contains(span: SourceSpan, offset: usize) -> bool {
    span.start <= offset && offset <= span.end
}

fn span_to_range(text: &str, span: SourceSpan) -> Option<Range> {
    Some(range_from_offsets(text, span.start, span.end))
}

fn range_from_offsets(text: &str, start: usize, end: usize) -> Range {
    Range {
        start: offset_to_position(text, start),
        end: offset_to_position(text, end),
    }
}

fn position_to_offset(text: &str, position: Position) -> Option<usize> {
    let line_index = position.line as usize;
    let character = position.character as usize;
    let starts = line_start_offsets(text);
    let line_start = *starts.get(line_index)?;
    let line_end = starts.get(line_index + 1).copied().unwrap_or(text.len());
    let line = &text[line_start..line_end];

    let mut offset = line_start;
    let mut utf16 = 0usize;
    for ch in line.chars() {
        if utf16 == character {
            return Some(offset);
        }
        utf16 += ch.len_utf16();
        offset += ch.len_utf8();
        if utf16 > character {
            return None;
        }
    }

    (utf16 == character).then_some(offset)
}

fn offset_to_position(text: &str, target: usize) -> Position {
    let bounded = target.min(text.len());
    let mut line = 0u32;
    let mut character = 0u32;

    for (offset, ch) in text.char_indices() {
        if offset >= bounded {
            break;
        }
        if ch == '\n' {
            line += 1;
            character = 0;
        } else {
            character += ch.len_utf16() as u32;
        }
    }

    Position { line, character }
}

fn line_start_offsets(text: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (offset, ch) in text.char_indices() {
        if ch == '\n' {
            starts.push(offset + ch.len_utf8());
        }
    }
    starts
}

fn word_at_offset(text: &str, offset: usize) -> Option<&str> {
    if text.is_empty() {
        return None;
    }
    let bytes = text.as_bytes();
    let mut start = offset.min(bytes.len().saturating_sub(1));
    if !is_word_byte(*bytes.get(start)?) {
        if start > 0 && is_word_byte(bytes[start - 1]) {
            start -= 1;
        } else {
            return None;
        }
    }
    let mut end = start;
    while start > 0 && is_word_byte(bytes[start - 1]) {
        start -= 1;
    }
    while end + 1 < bytes.len() && is_word_byte(bytes[end + 1]) {
        end += 1;
    }
    text.get(start..=end)
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::new(Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}

#[cfg(test)]
mod tests {
    use super::{
        analyze_document, declaration_span, document_symbols, locate_symbol, offset_to_position,
        position_to_offset, relation_target_span, type_reference_target_span, word_at_offset,
    };
    use tower_lsp::lsp_types::{Position, SymbolKind, Url};

    #[test]
    fn converts_utf16_positions_round_trip() {
        let text = "/// User docs\nmodel User {\n  emoji String\n}\n";
        let offset = position_to_offset(
            text,
            Position {
                line: 2,
                character: 2,
            },
        )
        .expect("position should resolve");

        assert_eq!(
            offset_to_position(text, offset),
            Position {
                line: 2,
                character: 2
            }
        );
    }

    #[test]
    fn returns_hoverable_symbol_docs_from_schema() {
        let text = "/// User docs\nmodel User {\n  /// Email docs\n  email String @id\n}\n";
        let uri = Url::parse("file:///schema.cstack").expect("uri should parse");
        let (schema, diagnostics) = analyze_document(&uri, text);

        assert!(diagnostics.is_empty());
        let schema = schema.expect("schema should parse");
        let offset = text.find("email").expect("field should exist");
        let symbol = locate_symbol(&schema, offset).expect("symbol should resolve");

        assert_eq!(symbol.kind, "field");
        assert_eq!(symbol.docs, vec!["Email docs".to_owned()]);
    }

    #[test]
    fn extracts_identifier_at_offset_for_definition_lookup() {
        let text = "model User {\n  userId Int @id\n}\n";
        let offset = text.find("userId").expect("identifier should exist") + 2;

        assert_eq!(word_at_offset(text, offset), Some("userId"));
    }

    #[test]
    fn resolves_declaration_span_by_name() {
        let text =
            "type FeedInput {\n  limit Int\n}\nprocedure getFeed(args: FeedInput): FeedInput\n";
        let uri = Url::parse("file:///schema.cstack").expect("uri should parse");
        let (schema, diagnostics) = analyze_document(&uri, text);

        assert!(diagnostics.is_empty());
        let schema = schema.expect("schema should parse");
        let span = declaration_span(&schema, "FeedInput").expect("type should resolve");

        assert_eq!(text[span.start..span.end].lines().next(), Some("FeedInput"));
    }

    #[test]
    fn builds_hierarchical_document_symbols() {
        let text = "model User {\n  id Int @id\n}\n";
        let uri = Url::parse("file:///schema.cstack").expect("uri should parse");
        let (schema, diagnostics) = analyze_document(&uri, text);

        assert!(diagnostics.is_empty());
        let schema = schema.expect("schema should parse");
        let symbols = document_symbols(text, &schema);

        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].kind, SymbolKind::STRUCT);
        assert_eq!(symbols[0].children.as_ref().expect("children").len(), 1);
    }

    #[test]
    fn resolves_relation_fields_and_references_to_the_correct_field_names() {
        let text = "model User {\n  id Int @id\n}\n\nmodel Post {\n  id Int @id\n  authorId Int\n  author User @relation(fields:[authorId],references:[id])\n}\n";
        let uri = Url::parse("file:///schema.cstack").expect("uri should parse");
        let (schema, diagnostics) = analyze_document(&uri, text);

        assert!(diagnostics.is_empty());
        let schema = schema.expect("schema should parse");

        let local_offset = text
            .rfind("authorId")
            .expect("relation authorId should exist");
        let reference_offset = text.rfind("id]").expect("reference id should exist");

        let local_span =
            relation_target_span(&schema, local_offset).expect("local field should resolve");
        let reference_span = relation_target_span(&schema, reference_offset)
            .expect("reference field should resolve");

        assert_eq!(&text[local_span.start..local_span.end], "authorId");
        assert_eq!(&text[reference_span.start..reference_span.end], "id");
        assert!(reference_span.start < local_span.start);
    }

    #[test]
    fn resolves_type_reference_to_declaration_name_span() {
        let text =
            "type FeedInput {\n  limit Int\n}\nprocedure getFeed(args: FeedInput): FeedInput\n";
        let uri = Url::parse("file:///schema.cstack").expect("uri should parse");
        let (schema, diagnostics) = analyze_document(&uri, text);

        assert!(diagnostics.is_empty());
        let schema = schema.expect("schema should parse");
        let offset = text.rfind("FeedInput").expect("return type should exist");
        let span =
            type_reference_target_span(&schema, offset).expect("type reference should resolve");

        assert_eq!(&text[span.start..span.end], "FeedInput");
        assert_eq!(
            span.start,
            text.find("FeedInput")
                .expect("type declaration should exist")
        );
    }

    #[test]
    fn narrows_unknown_relation_field_diagnostic_to_the_relation_name() {
        let text = "model User {\n  id Int @id\n}\n\nmodel Post {\n  id Int @id\n  authorId Int\n  author User @relation(fields:[ownerId],references:[id])\n}\n";
        let uri = Url::parse("file:///schema.cstack").expect("uri should parse");
        let (_schema, diagnostics) = analyze_document(&uri, text);

        assert_eq!(diagnostics.len(), 1);
        let diagnostic = &diagnostics[0];
        let start = position_to_offset(text, diagnostic.range.start).expect("start should resolve");
        let end = position_to_offset(text, diagnostic.range.end).expect("end should resolve");

        assert_eq!(&text[start..end], "ownerId");
    }

    #[test]
    fn includes_procedure_args_as_document_symbol_children() {
        let text =
            "/// Feed docs\n/// @param limit Maximum items\nprocedure getFeed(limit: Int): Int\n";
        let uri = Url::parse("file:///schema.cstack").expect("uri should parse");
        let (schema, diagnostics) = analyze_document(&uri, text);

        assert!(diagnostics.is_empty());
        let schema = schema.expect("schema should parse");
        let symbols = document_symbols(text, &schema);
        let args = symbols[0].children.as_ref().expect("children should exist");

        assert_eq!(args.len(), 1);
        assert_eq!(args[0].name, "limit");
        assert_eq!(args[0].kind, SymbolKind::VARIABLE);
    }
}
