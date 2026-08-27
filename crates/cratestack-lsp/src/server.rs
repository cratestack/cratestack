use tower_lsp_server::LanguageServer;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::{
    CompletionParams, CompletionResponse, DidChangeTextDocumentParams, DidOpenTextDocumentParams,
    DocumentHighlight, DocumentHighlightKind, DocumentHighlightParams, DocumentSymbolParams,
    DocumentSymbolResponse, GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverContents,
    HoverParams, InitializeParams, InitializeResult, InitializedParams, Location, MarkupContent,
    MarkupKind, MessageType, ReferenceParams, SemanticTokens, SemanticTokensParams,
    SemanticTokensResult, ServerInfo,
};

use crate::capabilities::server_capabilities;
use crate::completion::completion_items;
use crate::definition::definition_location;
use crate::document_symbols::document_symbols;
use crate::hover::locate_symbol;
use crate::hover_render::hover_markdown;
use crate::navigation::reference_ranges;
use crate::semantic_tokens::semantic_tokens;
use crate::state::Backend;
use crate::text::{position_to_offset, span_to_range};

impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            server_info: Some(ServerInfo {
                name: "cratestack-lsp".to_owned(),
                version: Some(env!("CARGO_PKG_VERSION").to_owned()),
            }),
            capabilities: server_capabilities(),
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
        let Some((text, schema)) = document.resolved() else {
            return Ok(None);
        };
        let Some(offset) = position_to_offset(text, text_document_position.position) else {
            return Ok(None);
        };
        let Some(symbol) = locate_symbol(schema, offset) else {
            return Ok(None);
        };
        Ok(Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: hover_markdown(&symbol, document.is_stale()),
            }),
            range: span_to_range(text, symbol.selection_span),
        }))
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let documents = self.documents.read().await;
        let schema = documents
            .get(&params.text_document_position.text_document.uri)
            .and_then(|document| document.resolved().map(|(_, schema)| schema));
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
        let Some((text, schema)) = document.resolved() else {
            return Ok(None);
        };
        let Some(offset) = position_to_offset(text, text_document_position.position) else {
            return Ok(None);
        };
        let Some(location) = definition_location(
            &text_document_position.text_document.uri,
            text,
            schema,
            offset,
        ) else {
            return Ok(None);
        };
        Ok(Some(GotoDefinitionResponse::Scalar(location)))
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let uri = params.text_document_position.text_document.uri;
        let documents = self.documents.read().await;
        let Some(document) = documents.get(&uri) else {
            return Ok(None);
        };
        Ok(reference_ranges(
            document,
            params.text_document_position.position,
            params.context.include_declaration,
        )
        .map(|ranges| {
            ranges
                .into_iter()
                .map(|range| Location {
                    uri: uri.clone(),
                    range,
                })
                .collect()
        }))
    }

    async fn document_highlight(
        &self,
        params: DocumentHighlightParams,
    ) -> Result<Option<Vec<DocumentHighlight>>> {
        let position = params.text_document_position_params;
        let documents = self.documents.read().await;
        let Some(document) = documents.get(&position.text_document.uri) else {
            return Ok(None);
        };
        Ok(
            reference_ranges(document, position.position, true).map(|ranges| {
                ranges
                    .into_iter()
                    .map(|range| DocumentHighlight {
                        range,
                        kind: Some(DocumentHighlightKind::TEXT),
                    })
                    .collect()
            }),
        )
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        let documents = self.documents.read().await;
        let Some(document) = documents.get(&params.text_document.uri) else {
            return Ok(None);
        };
        let Some((text, schema)) = document.resolved() else {
            return Ok(None);
        };
        Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
            result_id: None,
            data: semantic_tokens(text, schema),
        })))
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let documents = self.documents.read().await;
        let Some(document) = documents.get(&params.text_document.uri) else {
            return Ok(None);
        };
        let Some((text, schema)) = document.resolved() else {
            return Ok(None);
        };
        Ok(Some(DocumentSymbolResponse::Nested(document_symbols(
            text, schema,
        ))))
    }
}
