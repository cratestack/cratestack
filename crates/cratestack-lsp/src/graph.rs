//! Analysis of the whole schema a file belongs to (design section 12).
//!
//! A file that is part of a multi-file schema cannot be checked alone: a model
//! in `post.cstack` refers to types declared elsewhere. So the server loads the
//! graph the file is in, through its root, with every open buffer laid over the
//! disk, and attributes each error to the file it is in.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use cratestack_core::Schema;
use cratestack_parser::{
    LoadOptions, OverlaySources, SchemaFile, SchemaSources, canonical_schema_path,
    load_schema_diagnostics,
};
use tower_lsp_server::ls_types::{Diagnostic, Uri};

use crate::graph_resolver::Resolver;

/// The merged schema and the files it came from.
pub(crate) struct GraphView {
    pub(crate) schema: Schema,
    pub(crate) files: Vec<SchemaFile>,
    /// Every file's text at its base: what the schema's spans index into.
    pub(crate) merged: String,
    /// The URI each file is answered under: the one the editor opened it with,
    /// when it is open.
    uris: HashMap<PathBuf, Uri>,
}

impl GraphView {
    pub(crate) fn new(schema: Schema, files: Vec<SchemaFile>, uris: HashMap<PathBuf, Uri>) -> Self {
        let mut merged = String::new();
        for file in &files {
            // Bases leave one separator byte between files.
            while merged.len() < file.base {
                merged.push('\n');
            }
            merged.push_str(&file.source);
        }
        Self {
            schema,
            files,
            merged,
            uris,
        }
    }

    /// The URI to answer with for `file`.
    pub(crate) fn uri_of(&self, file: &SchemaFile) -> Option<Uri> {
        self.uris
            .get(&file.path)
            .cloned()
            .or_else(|| Uri::from_file_path(&file.path))
    }

    /// The file containing a global offset, and the offset within it.
    pub(crate) fn locate(&self, offset: usize) -> Option<(&SchemaFile, usize)> {
        self.files
            .iter()
            .find(|file| file.base <= offset && offset <= file.base + file.source.len())
            .map(|file| (file, offset - file.base))
    }
}

/// What loading one entry file found.
pub(crate) struct GraphAnalysis {
    /// The schema's root. For a graph that failed to load, the entry file.
    pub(crate) root: Uri,
    /// Every file that was read, paired with its canonical path.
    pub(crate) members: Vec<(Uri, PathBuf)>,
    /// One entry per member, empty when the member is clean.
    pub(crate) diagnostics: HashMap<Uri, Vec<Diagnostic>>,
    /// The merged schema, when the graph loaded.
    pub(crate) view: Option<Arc<GraphView>>,
}

/// Canonical path -> the URI the editor opened it under.
pub(crate) type OpenUris = HashMap<PathBuf, Uri>;

fn uri_for(path: &Path, open: &OpenUris) -> Option<Uri> {
    open.get(path)
        .cloned()
        .or_else(|| Uri::from_file_path(path))
}

/// Loads the graph containing `path` (given as `entry`) and reports every
/// problem against the file it is in.
pub(crate) fn analyze_graph(
    entry: &Uri,
    path: &Path,
    overlay: &OverlaySources,
    open: &OpenUris,
) -> GraphAnalysis {
    let options = LoadOptions {
        follow_part_of: true,
        sources: Some(overlay),
    };
    let load = load_schema_diagnostics(path, &options);
    let entry_path = canonical_schema_path(path);
    let named = load.named;
    let loaded = load.schema;
    let mut paths: Vec<PathBuf> = match &loaded {
        Some(loaded) => loaded.files.iter().map(|file| file.path.clone()).collect(),
        // Unreadable files (a missing root) are not files to publish to.
        None => load
            .files
            .into_iter()
            .filter(|path| overlay.read(path).is_ok())
            .collect(),
    };
    if !paths.contains(&entry_path) {
        paths.push(entry_path.clone());
    }
    let members: Vec<(Uri, PathBuf)> = paths
        .iter()
        .filter_map(|member| {
            let uri = if *member == entry_path {
                Some(entry.clone())
            } else {
                uri_for(member, open)
            };
            uri.map(|uri| (uri, member.clone()))
        })
        .collect();
    let uris: HashMap<PathBuf, Uri> = members
        .iter()
        .map(|(uri, path)| (path.clone(), uri.clone()))
        .collect();
    let root = loaded
        .as_ref()
        .and_then(|loaded| uris.get(&loaded.files[0].path))
        .cloned()
        .unwrap_or_else(|| entry.clone());
    let view = loaded.map(|loaded| Arc::new(GraphView::new(loaded.schema, loaded.files, uris)));
    let mut diagnostics: HashMap<Uri, Vec<Diagnostic>> = members
        .iter()
        .map(|(uri, _)| (uri.clone(), Vec::new()))
        .collect();
    let errors = load.errors;
    let resolver = Resolver {
        overlay,
        members: &members,
        named: &named,
        entry: (entry, entry_path),
    };
    for error in &errors {
        let (uri, diagnostic) = resolver.diagnostic(error);
        diagnostics.entry(uri).or_default().push(diagnostic);
    }
    GraphAnalysis {
        root,
        members,
        diagnostics,
        view,
    }
}
