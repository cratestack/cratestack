//! Analysing the entries of a refresh and recording the graphs found.

use cratestack_parser::{OverlaySources, canonical_schema_path};
use tower_lsp_server::ls_types::Uri;

use crate::graph::{GraphAnalysis, OpenUris, analyze_graph};
use crate::graph_store::{GraphRecord, GraphStore};

impl GraphStore {
    pub(super) fn analyse(
        &mut self,
        entries: Vec<Uri>,
        overlay: &OverlaySources,
        open: &OpenUris,
        affected: &mut Vec<Uri>,
    ) -> Vec<GraphAnalysis> {
        let mut analyses: Vec<GraphAnalysis> = Vec::new();
        for entry in entries {
            // An earlier entry already analysed this root.
            if analyses.iter().any(|done| done.root == entry) {
                continue;
            }
            let Some(path) = entry.to_file_path() else {
                continue;
            };
            let entry_path = canonical_schema_path(&path);
            // An unresolved graph started from a file that a resolved graph
            // now contains is that graph, found by another route.
            let covered = analyses.iter().any(|done| {
                done.view.is_some() && done.members.iter().any(|(_, p)| *p == entry_path)
            });
            if covered && self.records.get(&entry).is_some_and(|r| !r.resolved) {
                affected.extend(
                    self.records
                        .remove(&entry)
                        .into_iter()
                        .flat_map(|r| r.members),
                );
                continue;
            }
            let analysis = analyze_graph(&entry, &path, overlay, open);
            for key in [&entry, &analysis.root] {
                if let Some(old) = self.records.remove(key) {
                    affected.extend(old.members);
                }
            }
            affected.extend(analysis.members.iter().map(|(uri, _)| uri.clone()));
            self.records
                .insert(analysis.root.clone(), record_of(&analysis));
            analyses.push(analysis);
        }
        analyses
    }
}

fn record_of(analysis: &GraphAnalysis) -> GraphRecord {
    GraphRecord {
        members: analysis
            .members
            .iter()
            .map(|(uri, _)| uri.clone())
            .collect(),
        paths: analysis
            .members
            .iter()
            .map(|(_, path)| path.clone())
            .collect(),
        diagnostics: analysis
            .members
            .iter()
            .map(|(uri, path)| {
                let shown = analysis.diagnostics.get(uri).cloned().unwrap_or_default();
                (path.clone(), shown)
            })
            .collect(),
        resolved: analysis.view.is_some(),
    }
}
