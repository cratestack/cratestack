//! `GraphStore::close`, and what happens to a file that leaves a graph while
//! it is still open (cratestack#921).

use tower_lsp_server::ls_types::Diagnostic;

use crate::graph_store::{GraphStore, Refreshed};
use crate::tests_graph_support::{A, B, PART_OF, ROOT, root, sources, sources_open, uri};

fn shown<'a>(refreshed: &'a Refreshed, path: &str) -> Option<&'a [Diagnostic]> {
    refreshed
        .publish
        .iter()
        .find(|(uri_, _)| *uri_ == uri(path))
        .map(|(_, diagnostics)| diagnostics.as_slice())
}

fn bad_a() -> String {
    format!("{PART_OF}type A {{\n  x Nope\n}}\n")
}

fn good_a() -> String {
    format!("{PART_OF}type A {{\n  x Int\n}}\n")
}

#[test]
fn closing_a_part_with_unsaved_errors_reverts_to_its_disk_text() {
    let listing = root("part \"a.cstack\"\n");
    let (bad, good) = (bad_a(), good_a());
    let mut store = GraphStore::default();
    let (overlay, open) = sources(&[(ROOT, listing.as_str()), (A, bad.as_str())]);
    let first = store.refresh(&uri(A), &overlay, &open);
    assert_eq!(shown(&first, A).map(<[_]>::len), Some(1));

    // The buffer is gone; the disk (here, the overlay's fallback) has the good
    // text. The root is still open.
    let (overlay, open) = sources_open(&[(ROOT, listing.as_str()), (A, good.as_str())], &[ROOT]);
    let closed = store.close(&uri(A), &overlay, &open);
    assert_eq!(shown(&closed, A), Some(&[][..]), "a's stale error is gone");
    assert_eq!(closed.analyses.len(), 1, "the root is analysed again");
    assert!(closed.analyses[0].view.is_some());
}

#[test]
fn closing_the_last_open_file_of_a_graph_forgets_it() {
    let listing = root("part \"a.cstack\"\n");
    let mut store = GraphStore::default();
    let bad = bad_a();
    let files = [(ROOT, listing.as_str()), (A, bad.as_str())];
    let (overlay, open) = sources(&files);
    store.refresh(&uri(A), &overlay, &open);

    let (overlay, open) = sources_open(&files, &[ROOT]);
    store.close(&uri(A), &overlay, &open);
    let (overlay, open) = sources_open(&files, &[]);
    let closed = store.close(&uri(ROOT), &overlay, &open);
    assert_eq!(shown(&closed, ROOT), Some(&[][..]));
    assert_eq!(shown(&closed, A), Some(&[][..]), "nothing open shows it");
    assert!(store.members_of(&uri(ROOT)).is_none(), "no record is kept");
}

#[test]
fn closing_a_file_that_no_graph_knows_publishes_nothing() {
    let mut store = GraphStore::default();
    let (overlay, open) = sources_open(&[], &[]);
    let closed = store.close(&uri(A), &overlay, &open);
    assert_eq!(
        shown(&closed, A),
        Some(&[][..]),
        "whatever it showed is cleared"
    );
    assert_eq!(closed.publish.len(), 1);
    assert!(closed.analyses.is_empty());
}

#[test]
fn closing_the_root_keeps_a_graph_whose_part_is_still_open() {
    let listing = root("part \"a.cstack\"\n");
    let mut store = GraphStore::default();
    let bad = bad_a();
    let files = [(ROOT, listing.as_str()), (A, bad.as_str())];
    let (overlay, open) = sources(&files);
    store.refresh(&uri(A), &overlay, &open);

    let (overlay, open) = sources_open(&files, &[A]);
    let closed = store.close(&uri(ROOT), &overlay, &open);
    assert_eq!(
        shown(&closed, A).map(<[_]>::len),
        Some(1),
        "a still shows it"
    );
    // The graph was started from the part, so that is its key.
    assert!(store.members_of(&uri(A)).is_some());
}

fn dropped_files(b_text: &str) -> (String, String, [(&'static str, String); 3]) {
    let both = root("part \"a.cstack\"\npart \"b.cstack\"\n");
    let only_a = root("part \"a.cstack\"\n");
    let files = [
        (ROOT, only_a.clone()),
        (A, good_a()),
        (B, b_text.to_owned()),
    ];
    (both, only_a, files)
}

fn run_drop(b_text: &str, b_open: bool) -> (Refreshed, GraphStore) {
    let (both, _, files) = dropped_files(b_text);
    let view: Vec<(&str, &str)> = files.iter().map(|(p, t)| (*p, t.as_str())).collect();
    let mut store = GraphStore::default();
    let mut first = view.clone();
    first[0] = (ROOT, both.as_str());
    let (overlay, open) = sources(&first);
    store.refresh(&uri(ROOT), &overlay, &open);
    let opened: &[&str] = if b_open { &[ROOT, A, B] } else { &[ROOT, A] };
    let (overlay, open) = sources_open(&view, opened);
    let second = store.refresh(&uri(ROOT), &overlay, &open);
    (second, store)
}

#[test]
fn an_open_part_dropped_from_its_root_shows_m3() {
    let b = format!("{PART_OF}model B {{\n  id Int @id\n}}\n");
    let (second, _) = run_drop(&b, true);
    let in_b = shown(&second, B).expect("b is published to");
    assert_eq!(in_b.len(), 1, "{in_b:?}");
    assert_eq!(
        in_b[0].message,
        "`b.cstack` declares itself a part of `schema.cstack`, but `schema.cstack` does not list \
         it; add `part \"b.cstack\"` to `schema.cstack`"
    );
}

#[test]
fn an_open_file_that_is_no_longer_a_part_is_analysed_alone() {
    let b = "model B {\n  id Int @id\n  t Missing\n}\n";
    let (second, _) = run_drop(b, true);
    let in_b = shown(&second, B).expect("b is published to");
    assert_eq!(in_b.len(), 1, "{in_b:?}");
    assert_eq!(in_b[0].message, "unknown type `Missing`");
}

#[test]
fn a_dropped_file_that_is_not_open_is_cleared() {
    let b = format!("{PART_OF}model B {{\n  id Int @id\n  t Missing\n}}\n");
    let (second, store) = run_drop(&b, false);
    assert_eq!(shown(&second, B), Some(&[][..]));
    assert!(!store.members_of(&uri(ROOT)).unwrap().contains(&uri(B)));
}
