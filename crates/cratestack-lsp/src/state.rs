use std::collections::HashMap;
use std::sync::Arc;

use cratestack_core::{Schema, SourceSpan};
use tokio::sync::RwLock;
use tower_lsp_server::Client;
use tower_lsp_server::ls_types::Uri;

use crate::graph::GraphView;
use crate::graph_store::GraphStore;
use crate::view::Resolved;

/// A schema together with the exact source text it was parsed from.
///
/// The pairing is the whole point. Spans index into the text that produced
/// them, so a schema is only meaningful alongside that text — mixing a retained
/// schema with newer document text would resolve offsets against bytes the
/// parser never saw.
#[derive(Clone)]
pub(crate) struct AnalyzedSchema {
    pub(crate) text: String,
    pub(crate) schema: Schema,
    /// Offset of this file's first byte in the schema's address space: spans
    /// in `schema` are global. Zero for a single-file schema.
    pub(crate) base: usize,
    /// The whole graph, for a schema split across files.
    pub(crate) graph: Option<Arc<GraphView>>,
}

#[derive(Clone)]
pub(crate) struct DocumentState {
    /// What the editor currently holds. Diagnostics are reported against this.
    pub(crate) text: String,
    /// The most recent text that *parsed*, and its schema. Retained across a
    /// failed parse so navigation, hover and colouring survive the moment
    /// between two valid states — which, while someone is typing, is most
    /// moments. Dropping it (the previous behaviour) made every feature that
    /// needs a schema go silent on a single stray character.
    pub(crate) analyzed: Option<AnalyzedSchema>,
}

impl DocumentState {
    /// Source text and schema that agree with each other. In a part of a
    /// split schema the spans are global and the text is the file's, so only
    /// [`Self::view`] is safe to compare them with.
    ///
    /// Callers must take *both* from here rather than reaching for
    /// `DocumentState::text`: after a failed parse these are the older,
    /// self-consistent pair, and resolving a position against one while
    /// reading spans from the other yields nonsense.
    pub(crate) fn resolved(&self) -> Option<(&str, &Schema)> {
        self.analyzed
            .as_ref()
            .map(|analyzed| (analyzed.text.as_str(), &analyzed.schema))
    }

    /// The same pair as [`Self::resolved`], with the file's place in the
    /// schema's address space. Features that compare positions with schema
    /// spans must use this: in a part, `resolved()`'s text and spans disagree.
    pub(crate) fn view(&self) -> Option<Resolved<'_>> {
        let analyzed = self.analyzed.as_ref()?;
        let graph = analyzed.graph.as_deref();
        Some(Resolved {
            text: &analyzed.text,
            schema: &analyzed.schema,
            base: analyzed.base,
            graph,
            global: graph.map_or(analyzed.text.as_str(), |graph| graph.merged.as_str()),
        })
    }

    /// Whether the schema spans more than one file.
    pub(crate) fn is_split(&self) -> bool {
        self.analyzed
            .as_ref()
            .is_some_and(|analyzed| analyzed.graph.is_some())
    }

    /// Whether the text carries a `part`, `part of` or `import` line, which
    /// makes it one file of a split schema even when no graph was loaded for
    /// it (a shared import opened alone, or a graph that failed to load).
    pub(crate) fn has_directive(&self) -> bool {
        self.text.lines().any(|line| {
            ["part of", "part", "import"].iter().any(|keyword| {
                line.strip_prefix(keyword).is_some_and(|rest| {
                    rest.trim_start().starts_with('"') && rest.starts_with([' ', '\t', '"'])
                })
            })
        })
    }

    /// Whether the retained schema predates the current text — i.e. results are
    /// being served from a stale parse.
    pub(crate) fn is_stale(&self) -> bool {
        self.analyzed
            .as_ref()
            .is_some_and(|analyzed| analyzed.text != self.text)
    }
}

/// Folds a fresh parse into whatever was known before.
///
/// Split out of `Backend::update_document` so the retention rule is testable
/// without standing up an LSP `Client`.
pub(crate) fn next_document_state(
    previous: Option<DocumentState>,
    text: String,
    schema: Option<Schema>,
) -> DocumentState {
    let analyzed = match schema {
        Some(schema) => Some(AnalyzedSchema {
            text: text.clone(),
            schema,
            base: 0,
            graph: None,
        }),
        // Parse failed. Carry the last good pair forward untouched — including
        // its text, so its spans stay resolvable. A document that has never
        // parsed keeps `None`; there is no schema to fall back to and inventing
        // one would be worse than staying quiet.
        None => previous.and_then(|state| state.analyzed),
    };
    DocumentState { text, analyzed }
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
    /// Every analysed schema graph and who is in it. Lock order: `documents`
    /// is never held while waiting on `graphs`.
    pub(crate) graphs: Arc<RwLock<GraphStore>>,
}

impl Backend {
    pub(crate) fn new(client: Client) -> Self {
        Self {
            client,
            documents: Arc::new(RwLock::new(HashMap::new())),
            graphs: Arc::new(RwLock::new(GraphStore::default())),
        }
    }
}
