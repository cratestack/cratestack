//! A document was closed: its buffer stops overlaying the disk.

use cratestack_parser::OverlaySources;
use tower_lsp_server::ls_types::Uri;

use crate::graph::OpenUris;
use crate::graph_store::{GraphStore, Refreshed, canon};

impl GraphStore {
    /// `uri` was closed; `overlay` and `open` no longer have it. Every graph
    /// it was in is read again, from the disk for the closed file. A graph
    /// with no open file left is forgotten, and its files show nothing: a file
    /// that is not open and in no graph has no diagnostics.
    pub(crate) fn close(
        &mut self,
        uri: &Uri,
        overlay: &OverlaySources,
        open: &OpenUris,
    ) -> Refreshed {
        let mut affected = vec![uri.clone()];
        let mut keep = Vec::new();
        if let Some(path) = canon(uri) {
            for root in self.roots_containing(&path) {
                let still_open = self.records[&root]
                    .paths
                    .iter()
                    .any(|member| open.contains_key(member));
                if still_open {
                    keep.push(root);
                } else if let Some(record) = self.records.remove(&root) {
                    affected.extend(record.members);
                }
            }
        }
        self.run(keep, overlay, open, affected)
    }
}
