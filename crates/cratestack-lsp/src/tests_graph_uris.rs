//! A file is answered under the URI the editor opened it with, and graph
//! membership is a question about files, not spellings of their URIs
//! (cratestack#921). Needs a real symlinked directory.
#![cfg(unix)]

use std::path::PathBuf;

use cratestack_parser::{OverlaySources, canonical_schema_path};
use tower_lsp_server::ls_types::Uri;

use crate::graph::{OpenUris, analyze_graph};
use crate::graph_store::GraphStore;
use crate::tests_graph_support::HEAD;

struct Tree(PathBuf);

impl Tree {
    fn new(name: &str) -> Self {
        let base = std::fs::canonicalize(std::env::temp_dir()).unwrap();
        let dir = base.join(format!("cratestack-lsp-uris-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("real")).unwrap();
        std::os::unix::fs::symlink(dir.join("real"), dir.join("alias")).unwrap();
        std::fs::write(
            dir.join("real/schema.cstack"),
            format!("{HEAD}import \"money.cstack\"\n\nmodel W {{\n  id Int @id\n  m Money\n}}\n"),
        )
        .unwrap();
        std::fs::write(dir.join("real/money.cstack"), "enum Money {\n  X\n}\n").unwrap();
        Tree(dir)
    }

    fn uri(&self, rel: &str) -> Uri {
        Uri::from_file_path(self.0.join(rel)).unwrap()
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn open_under(tree: &Tree, rels: &[&str]) -> OpenUris {
    rels.iter()
        .map(|rel| (canonical_schema_path(&tree.0.join(rel)), tree.uri(rel)))
        .collect()
}

#[test]
fn other_files_are_answered_under_the_uri_the_editor_opened() {
    let tree = Tree::new("answer");
    let open = open_under(&tree, &["real/schema.cstack", "alias/money.cstack"]);
    let analysis = analyze_graph(
        &tree.uri("real/schema.cstack"),
        &tree.0.join("real/schema.cstack"),
        &OverlaySources::new(true),
        &open,
    );
    let view = analysis.view.expect("loads");
    let money = view
        .files
        .iter()
        .find(|f| f.relative == "money.cstack")
        .unwrap();
    assert_eq!(view.uri_of(money), Some(tree.uri("alias/money.cstack")));
}

#[test]
fn membership_ignores_the_spelling_of_a_uri() {
    let tree = Tree::new("member");
    let mut store = GraphStore::default();
    let overlay = OverlaySources::new(true);
    let root = tree.uri("real/schema.cstack");
    // The root is analysed with money closed: it is recorded by its real URI.
    store.refresh(&root, &overlay, &open_under(&tree, &["real/schema.cstack"]));

    // Money is opened through the symlinked directory.
    let money = tree.uri("alias/money.cstack");
    let open = open_under(&tree, &["real/schema.cstack", "alias/money.cstack"]);
    let refreshed = store.refresh(&money, &overlay, &open);
    assert!(
        refreshed.analyses.iter().any(|a| a.root == root),
        "the importer is refreshed too"
    );
    assert!(refreshed.publish.iter().any(|(uri, _)| *uri == money));
}
