//! What the server remembers about the schema graphs it has analysed, and the
//! rule for refreshing them. Pure data and functions: no `Client`, so the
//! whole rule is testable without a running server.
//!
//! Membership is a question about files, so it is answered by canonical path:
//! the URI an editor opened a file under need not be the one a graph first
//! recorded it under.

use std::collections::HashMap;
use std::path::PathBuf;

use cratestack_parser::{OverlaySources, canonical_schema_path};
use tower_lsp_server::ls_types::{Diagnostic, Uri};

use crate::graph::{GraphAnalysis, OpenUris};

/// One analysed graph: who is in it, and what each member was told.
pub(crate) struct GraphRecord {
    pub(crate) members: Vec<Uri>,
    /// Canonical paths of `members`, in the same order.
    pub(crate) paths: Vec<PathBuf>,
    pub(crate) diagnostics: HashMap<PathBuf, Vec<Diagnostic>>,
    /// Whether the graph loaded. A graph that did not is keyed by the file it
    /// was started from, which may be a part of some other root.
    pub(crate) resolved: bool,
}

/// The outcome of a refresh.
pub(crate) struct Refreshed {
    /// What to publish: every file whose diagnostics may have changed, with
    /// the full list it should now show (empty clears it).
    pub(crate) publish: Vec<(Uri, Vec<Diagnostic>)>,
    /// The fresh analyses, for updating the open documents' schemas.
    pub(crate) analyses: Vec<GraphAnalysis>,
}

/// Every graph analysed so far, keyed by root (by the entry file for a graph
/// whose root could not be found).
#[derive(Default)]
pub(crate) struct GraphStore {
    pub(super) records: HashMap<Uri, GraphRecord>,
}

pub(super) fn canon(uri: &Uri) -> Option<PathBuf> {
    uri.to_file_path().map(|path| canonical_schema_path(&path))
}

impl GraphStore {
    /// The recorded members of the graph rooted at `root`.
    #[cfg(test)]
    pub(crate) fn members_of(&self, root: &Uri) -> Option<&[Uri]> {
        self.records
            .get(root)
            .map(|record| record.members.as_slice())
    }

    /// Re-analyses the graph `uri` is in, and every other recorded graph that
    /// contains `uri` (an edit to a shared import refreshes each importer).
    pub(crate) fn refresh(
        &mut self,
        uri: &Uri,
        overlay: &OverlaySources,
        open: &OpenUris,
    ) -> Refreshed {
        let mut entries = vec![uri.clone()];
        if let Some(path) = canon(uri) {
            entries.extend(
                self.roots_containing(&path)
                    .into_iter()
                    .filter(|root| canon(root).as_ref() != Some(&path)),
            );
        }
        self.run(entries, overlay, open, Vec::new())
    }

    /// The roots of the recorded graphs that contain `path`, in a fixed order.
    pub(super) fn roots_containing(&self, path: &PathBuf) -> Vec<Uri> {
        let mut roots: Vec<Uri> = self
            .records
            .iter()
            .filter(|(_, record)| record.paths.contains(path))
            .map(|(root, _)| root.clone())
            .collect();
        roots.sort_by_key(|root| root.to_string());
        roots
    }

    pub(super) fn run(
        &mut self,
        entries: Vec<Uri>,
        overlay: &OverlaySources,
        open: &OpenUris,
        mut affected: Vec<Uri>,
    ) -> Refreshed {
        let mut analyses = self.analyse(entries, overlay, open, &mut affected);
        // A file that left every graph while it is open is its own schema now
        // (or still a part of a root that no longer lists it, which is M3).
        let mut dropped: Vec<Uri> = Vec::new();
        for uri in &affected {
            let Some(path) = canon(uri) else { continue };
            let in_graph = self.records.values().any(|r| r.paths.contains(&path));
            if open.contains_key(&path) && !in_graph && !dropped.contains(uri) {
                dropped.push(uri.clone());
            }
        }
        for uri in dropped {
            analyses.extend(self.analyse(vec![uri], overlay, open, &mut affected));
        }
        let mut seen: Vec<Uri> = Vec::new();
        let mut publish = Vec::new();
        for uri in affected {
            if seen.contains(&uri) {
                continue;
            }
            publish.push((uri.clone(), self.diagnostics_for(&uri)));
            seen.push(uri);
        }
        Refreshed { publish, analyses }
    }

    /// What a file shows: the diagnostics of every graph it is in, each once.
    fn diagnostics_for(&self, uri: &Uri) -> Vec<Diagnostic> {
        let Some(path) = canon(uri) else {
            return Vec::new();
        };
        let mut all: Vec<Diagnostic> = Vec::new();
        let mut roots: Vec<&Uri> = self.records.keys().collect();
        roots.sort_by_key(|root| root.to_string());
        for root in roots {
            for diagnostic in self.records[root]
                .diagnostics
                .get(&path)
                .into_iter()
                .flatten()
            {
                if !all.contains(diagnostic) {
                    all.push(diagnostic.clone());
                }
            }
        }
        all
    }
}
