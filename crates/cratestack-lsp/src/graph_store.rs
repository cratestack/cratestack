//! What the server remembers about the schema graphs it has analysed, and the
//! rule for refreshing them. Pure data and functions: no `Client`, so the
//! whole rule is testable without a running server.

use std::collections::HashMap;

use cratestack_parser::OverlaySources;
use tower_lsp_server::ls_types::{Diagnostic, Uri};

use crate::graph::{GraphAnalysis, OpenUris, analyze_graph};

/// One analysed graph: who is in it, and what each member was told.
pub(crate) struct GraphRecord {
    pub(crate) members: Vec<Uri>,
    pub(crate) diagnostics: HashMap<Uri, Vec<Diagnostic>>,
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
    records: HashMap<Uri, GraphRecord>,
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
        let mut others: Vec<Uri> = self
            .records
            .iter()
            .filter(|(root, record)| record.members.contains(uri) && *root != uri)
            .map(|(root, _)| root.clone())
            .collect();
        others.sort_by_key(|root| root.to_string());
        entries.extend(others);

        let mut affected: Vec<Uri> = Vec::new();
        let mut analyses: Vec<GraphAnalysis> = Vec::new();
        for entry in entries {
            // An earlier entry already analysed this root.
            if analyses.iter().any(|done| done.root == entry) {
                continue;
            }
            // An unresolved graph started from a file that a resolved graph
            // now contains is that graph, found by another route.
            let covered = analyses
                .iter()
                .any(|done| done.view.is_some() && done.members.iter().any(|(m, _)| *m == entry));
            if covered && self.records.get(&entry).is_some_and(|r| !r.resolved) {
                affected.extend(
                    self.records
                        .remove(&entry)
                        .into_iter()
                        .flat_map(|r| r.members),
                );
                continue;
            }
            let Some(path) = entry.to_file_path() else {
                continue;
            };
            let analysis = analyze_graph(&entry, &path, overlay, open);
            for key in [&entry, &analysis.root] {
                if let Some(old) = self.records.remove(key) {
                    affected.extend(old.members);
                }
            }
            affected.extend(analysis.members.iter().map(|(uri, _)| uri.clone()));
            self.records.insert(
                analysis.root.clone(),
                GraphRecord {
                    members: analysis
                        .members
                        .iter()
                        .map(|(uri, _)| uri.clone())
                        .collect(),
                    diagnostics: analysis.diagnostics.clone(),
                    resolved: analysis.view.is_some(),
                },
            );
            analyses.push(analysis);
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
        let mut all: Vec<Diagnostic> = Vec::new();
        let mut roots: Vec<&Uri> = self.records.keys().collect();
        roots.sort_by_key(|root| root.to_string());
        for root in roots {
            for diagnostic in self.records[root]
                .diagnostics
                .get(uri)
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
