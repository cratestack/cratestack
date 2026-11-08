//! Fixtures of the multi-file diagnostics suites: schemas laid over fake
//! absolute paths, so nothing touches the disk.

use std::path::Path;

use cratestack_parser::{OverlaySources, canonical_schema_path};
use tower_lsp_server::ls_types::{Diagnostic, Uri};

use crate::graph::{GraphAnalysis, OpenUris, analyze_graph};

pub(super) const ROOT: &str = "/fixture/schema.cstack";
pub(super) const A: &str = "/fixture/a.cstack";
pub(super) const B: &str = "/fixture/b.cstack";
pub(super) const HEAD: &str = "datasource db {\n  provider = \"postgresql\"\n  url = env(\"DATABASE_URL\")\n}\n\nauth Caller {\n  id Int\n}\n\n";
pub(super) const PART_OF: &str = "part of \"schema.cstack\"\n\n";

pub(super) fn uri(path: &str) -> Uri {
    Uri::from_file_path(path).expect("fixture paths are absolute")
}

pub(super) fn root(listing: &str) -> String {
    format!("{HEAD}{listing}")
}

/// The loader's input for these files, and the URIs they are "open" under.
pub(super) fn sources(files: &[(&str, &str)]) -> (OverlaySources, OpenUris) {
    let mut overlay = OverlaySources::new(true);
    let mut open = OpenUris::new();
    for (path, text) in files {
        overlay.insert(path, *text);
        open.insert(canonical_schema_path(Path::new(path)), uri(path));
    }
    (overlay, open)
}

/// Like [`sources`], but only `open` of the files are open buffers: the rest
/// are read as if from the disk.
pub(super) fn sources_open(
    files: &[(&str, &str)],
    open_paths: &[&str],
) -> (OverlaySources, OpenUris) {
    let (overlay, _) = sources(files);
    let open = open_paths
        .iter()
        .map(|path| (canonical_schema_path(Path::new(path)), uri(path)))
        .collect();
    (overlay, open)
}

pub(super) fn analyze(entry: &str, files: &[(&str, &str)]) -> GraphAnalysis {
    let (overlay, open) = sources(files);
    analyze_graph(&uri(entry), Path::new(entry), &overlay, &open)
}

pub(super) fn diagnostics_of<'a>(analysis: &'a GraphAnalysis, path: &str) -> &'a [Diagnostic] {
    analysis
        .diagnostics
        .get(&uri(path))
        .unwrap_or_else(|| panic!("{path} should be a member"))
}
