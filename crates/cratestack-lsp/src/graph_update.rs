//! `Backend::update_document`: keep the open documents' texts, analyse the
//! graph each belongs to, and publish diagnostics per file.

use std::collections::HashMap;
use std::sync::Arc;

use cratestack_parser::{OverlaySources, canonical_schema_path};
use tower_lsp_server::ls_types::Uri;

use crate::analyze::analyze_document;
use crate::graph::{GraphAnalysis, OpenUris};
use crate::state::{AnalyzedSchema, Backend, DocumentState, next_document_state};

/// Every open document with a file path, as loader input: its text laid over
/// the disk, and the URI it was opened under.
pub(crate) fn open_sources(documents: &HashMap<Uri, DocumentState>) -> (OverlaySources, OpenUris) {
    let mut overlay = OverlaySources::new(true);
    let mut open = OpenUris::new();
    for (uri, state) in documents {
        if let Some(path) = uri.to_file_path() {
            overlay.insert(&path, state.text.as_str());
            open.insert(canonical_schema_path(&path), uri.clone());
        }
    }
    (overlay, open)
}

/// Gives every open member the schema its graph produced. A graph that failed
/// to load leaves the members' last good schema alone.
pub(crate) fn apply_analyses(
    documents: &mut HashMap<Uri, DocumentState>,
    analyses: &[GraphAnalysis],
) {
    for analysis in analyses {
        let Some(view) = &analysis.view else {
            continue;
        };
        for (uri, path) in &analysis.members {
            let Some(document) = documents.get_mut(uri) else {
                continue;
            };
            let Some(file) = view.files.iter().find(|file| file.path == *path) else {
                continue;
            };
            document.analyzed = Some(AnalyzedSchema {
                text: file.source.to_string(),
                schema: view.schema.clone(),
                base: file.base,
                graph: (view.files.len() > 1).then(|| Arc::clone(view)),
            });
        }
    }
}

impl Backend {
    pub(crate) async fn update_document(&self, uri: Uri, text: String) {
        if uri.to_file_path().is_none() {
            self.update_untitled(uri, text).await;
            return;
        }
        let (overlay, open) = {
            let mut documents = self.documents.write().await;
            let previous = documents.remove(&uri);
            documents.insert(
                uri.clone(),
                DocumentState {
                    text,
                    analyzed: previous.and_then(|state| state.analyzed),
                },
            );
            open_sources(&documents)
        };
        let refreshed = self.graphs.write().await.refresh(&uri, &overlay, &open);
        apply_analyses(&mut *self.documents.write().await, &refreshed.analyses);
        for (uri, diagnostics) in refreshed.publish {
            self.client
                .publish_diagnostics(uri, diagnostics, None)
                .await;
        }
    }

    /// The editor closed `uri`: its buffer stops overlaying the disk, and
    /// every graph it was in is read again without it.
    pub(crate) async fn close_document(&self, uri: Uri) {
        let (overlay, open) = {
            let mut documents = self.documents.write().await;
            documents.remove(&uri);
            open_sources(&documents)
        };
        if uri.to_file_path().is_none() {
            // A buffer with no file path has no graph and nothing on disk.
            self.client.publish_diagnostics(uri, Vec::new(), None).await;
            return;
        }
        let refreshed = self.graphs.write().await.close(&uri, &overlay, &open);
        apply_analyses(&mut *self.documents.write().await, &refreshed.analyses);
        for (uri, diagnostics) in refreshed.publish {
            self.client
                .publish_diagnostics(uri, diagnostics, None)
                .await;
        }
    }

    /// A buffer with no file path has no graph: it is its own schema.
    async fn update_untitled(&self, uri: Uri, text: String) {
        let (schema, diagnostics) = analyze_document(&uri, &text);
        {
            let mut documents = self.documents.write().await;
            let previous = documents.remove(&uri);
            documents.insert(uri.clone(), next_document_state(previous, text, schema));
        }
        // Diagnostics always describe the *current* text, never the retained
        // schema — a stale parse must not suppress a live error.
        self.client
            .publish_diagnostics(uri, diagnostics, None)
            .await;
    }
}
