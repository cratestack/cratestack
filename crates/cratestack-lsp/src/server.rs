use tower_lsp_server::LanguageServer;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::{
    CompletionOptions, CompletionParams, CompletionResponse, DidChangeTextDocumentParams,
    DidOpenTextDocumentParams, DocumentSymbolParams, DocumentSymbolResponse, GotoDefinitionParams,
    GotoDefinitionResponse, Hover, HoverContents, HoverParams, HoverProviderCapability,
    InitializeParams, InitializeResult, InitializedParams, MarkupContent, MarkupKind, MessageType,
    OneOf, ServerCapabilities, ServerInfo, TextDocumentSyncCapability, TextDocumentSyncKind,
};

use crate::completion::completion_items;
use crate::definition::definition_location;
use crate::document_symbols::document_symbols;
use crate::hover::locate_symbol;
use crate::state::Backend;
use crate::text::{position_to_offset, span_to_range};

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
            offset_encoding: None,
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
