use std::collections::HashMap;
use std::sync::Arc;

use cratestack_core::{Schema, SourceSpan};
use tokio::sync::RwLock;
use tower_lsp_server::Client;
use tower_lsp_server::ls_types::Uri;

use crate::analyze::analyze_document;

#[derive(Clone)]
pub(crate) struct DocumentState {
    pub(crate) text: String,
    pub(crate) schema: Option<Schema>,
}

#[derive(Clone)]
pub(crate) struct SymbolInfo {
    pub(crate) kind: &'static str,
    pub(crate) name: String,
    pub(crate) detail: String,
    pub(crate) docs: Vec<String>,
    pub(crate) selection_span: SourceSpan,
}

#[derive(Clone)]
pub(crate) struct SpannedName {
    pub(crate) name: String,
    pub(crate) span: SourceSpan,
}

pub(crate) struct ParsedRelationAttributeSpans {
    pub(crate) fields: Vec<SpannedName>,
    pub(crate) references: Vec<SpannedName>,
}

pub(crate) struct Backend {
    pub(crate) client: Client,
    pub(crate) documents: Arc<RwLock<HashMap<Uri, DocumentState>>>,
}

impl Backend {
    pub(crate) fn new(client: Client) -> Self {
        Self {
            client,
            documents: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub(crate) async fn update_document(&self, uri: Uri, text: String) {
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
