//! Fixtures of the multi-file navigation suites: a schema split across three
//! files, opened the way the server opens it.

use std::collections::HashMap;

use tower_lsp_server::ls_types::{Position, Range, Uri};

use crate::graph_update::apply_analyses;
use crate::state::DocumentState;
use crate::tests_graph_support::{A, B, PART_OF, ROOT, analyze, root, uri};
use crate::text::{offset_to_position, position_to_offset};

pub(super) const MONEY: &str = "/fixture/money.cstack";

pub(super) struct Fixture {
    pub(super) files: Vec<(&'static str, String)>,
    pub(super) documents: HashMap<Uri, DocumentState>,
}

impl Fixture {
    pub(super) fn text(&self, path: &str) -> &str {
        &self.files.iter().find(|(p, _)| *p == path).unwrap().1
    }

    pub(super) fn document(&self, path: &str) -> &DocumentState {
        &self.documents[&uri(path)]
    }

    /// `range` of `path`'s text.
    pub(super) fn slice(&self, path: &str, range: Range) -> &str {
        let text = self.text(path);
        let start = position_to_offset(text, range.start).unwrap();
        let end = position_to_offset(text, range.end).unwrap();
        &text[start..end]
    }

    /// The position of the `occurrence`-th (0-based) `needle` in `path`.
    pub(super) fn position(&self, path: &str, needle: &str, occurrence: usize) -> Position {
        let text = self.text(path);
        let offset = text.match_indices(needle).nth(occurrence).unwrap().0;
        offset_to_position(text, offset + 1)
    }
}

/// Opens every file of the graph and analyzes it from `entry`.
pub(super) fn open(entry: &str, files: Vec<(&'static str, String)>) -> Fixture {
    let borrowed: Vec<(&str, &str)> = files.iter().map(|(p, t)| (*p, t.as_str())).collect();
    let analysis = analyze(entry, &borrowed);
    let mut documents: HashMap<Uri, DocumentState> = files
        .iter()
        .map(|(path, text)| {
            let state = DocumentState {
                text: text.clone(),
                analyzed: None,
            };
            (uri(path), state)
        })
        .collect();
    assert!(
        analysis.view.is_some(),
        "fixture should load: {:?}",
        analysis.diagnostics
    );
    apply_analyses(&mut documents, &[analysis]);
    Fixture { files, documents }
}

/// `a.cstack` and `b.cstack` are parts of `schema.cstack`; `b` declares `B`,
/// which `a` and `b` both use.
pub(super) fn split() -> Fixture {
    let root = root("part \"a.cstack\"\npart \"b.cstack\"\n");
    let a = format!("{PART_OF}/// Account owner.\nmodel A {{\n  id Int @id\n  b B\n}}\n");
    let b = format!(
        "{PART_OF}/// The kind.\nenum B {{\n  X\n}}\n\nmodel C {{\n  id Int @id\n  b B\n}}\n"
    );
    open(ROOT, vec![(ROOT, root), (A, a), (B, b)])
}

/// `schema.cstack` imports `money.cstack` and lists the part `a.cstack`,
/// whose model uses the imported `Currency`.
pub(super) fn with_import() -> Fixture {
    let root = root("import \"money.cstack\"\npart \"a.cstack\"\n");
    let money = "/// A currency.\nenum Currency {\n  USD\n}\n\nmixin Stamps {\n  at DateTime\n}\n"
        .to_owned();
    let a = format!("{PART_OF}model A {{\n  id Int @id\n  cur Currency\n}}\n");
    open(ROOT, vec![(ROOT, root), (MONEY, money), (A, a)])
}
