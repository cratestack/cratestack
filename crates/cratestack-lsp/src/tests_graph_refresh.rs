//! The refresh rule of `GraphStore`: which files are re-analysed and
//! re-published when one file changes (cratestack#921).

use tower_lsp_server::ls_types::Diagnostic;

use crate::graph_store::{GraphStore, Refreshed};
use crate::tests_graph_support::{A, B, PART_OF, ROOT, root, sources, uri};

const R1: &str = "/fixture/r1.cstack";
const R2: &str = "/fixture/r2.cstack";
const MONEY: &str = "/fixture/money.cstack";

fn refresh(store: &mut GraphStore, entry: &str, files: &[(&str, &str)]) -> Refreshed {
    let (overlay, open) = sources(files);
    store.refresh(&uri(entry), &overlay, &open)
}

/// What was published to `path`, if anything was.
fn shown<'a>(refreshed: &'a Refreshed, path: &str) -> Option<&'a [Diagnostic]> {
    refreshed
        .publish
        .iter()
        .find(|(uri_, _)| *uri_ == uri(path))
        .map(|(_, diagnostics)| diagnostics.as_slice())
}

#[test]
fn editing_a_part_refreshes_its_siblings() {
    let root = root("part \"a.cstack\"\npart \"b.cstack\"\n");
    let a_bad = format!("{PART_OF}enum Thing2 {{\n  X\n}}\n");
    let a_good = format!("{PART_OF}enum Thing {{\n  X\n}}\n");
    let b = format!("{PART_OF}model B {{\n  id Int @id\n  t Thing\n}}\n");
    let mut store = GraphStore::default();

    let first = refresh(
        &mut store,
        B,
        &[(ROOT, root.as_str()), (A, a_bad.as_str()), (B, b.as_str())],
    );
    let in_b = shown(&first, B).expect("b is published to");
    assert_eq!(in_b.len(), 1, "{in_b:?}");
    assert_eq!(in_b[0].message, "unknown type `Thing`");

    // Fixing A is an edit to A; B's diagnostic is B's to lose.
    let second = refresh(
        &mut store,
        A,
        &[(ROOT, root.as_str()), (A, a_good.as_str()), (B, b.as_str())],
    );
    assert_eq!(
        shown(&second, B),
        Some(&[][..]),
        "b's stale error is cleared"
    );
    assert_eq!(second.analyses.len(), 1, "one root, analysed once");
    assert!(second.analyses[0].view.is_some());
}

#[test]
fn editing_an_import_refreshes_its_importer() {
    let importer = |name: &str| {
        let _ = name;
        root("import \"money.cstack\"\n\nmodel Wallet {\n  id Int @id\n  m Money\n}\n")
    };
    let (r1, r2) = (importer("r1"), importer("r2"));
    let without = "enum Cash {\n  X\n}\n";
    let with = "enum Money {\n  X\n}\n";
    let mut store = GraphStore::default();

    let files = |money: &'static str| {
        vec![
            (R1, r1.clone()),
            (R2, r2.clone()),
            (MONEY, money.to_owned()),
        ]
    };
    let owned = files(without);
    let view: Vec<(&str, &str)> = owned.iter().map(|(p, t)| (*p, t.as_str())).collect();
    refresh(&mut store, R1, &view);
    refresh(&mut store, R2, &view);
    let bad = refresh(&mut store, MONEY, &view);
    assert_eq!(shown(&bad, R1).map(<[_]>::len), Some(1));
    assert_eq!(shown(&bad, R2).map(<[_]>::len), Some(1));

    // Editing the shared file, which is not itself a member of either root's
    // own record key, refreshes both importers.
    let owned = files(with);
    let view: Vec<(&str, &str)> = owned.iter().map(|(p, t)| (*p, t.as_str())).collect();
    let fixed = refresh(&mut store, MONEY, &view);
    assert_eq!(shown(&fixed, R1), Some(&[][..]));
    assert_eq!(shown(&fixed, R2), Some(&[][..]));
    assert!(
        store
            .members_of(&uri(R1))
            .expect("r1 recorded")
            .contains(&uri(MONEY))
    );
}

#[test]
fn a_file_leaving_the_graph_gets_its_diagnostics_cleared() {
    let both = root("part \"a.cstack\"\npart \"b.cstack\"\n");
    let only_a = root("part \"a.cstack\"\n");
    let a = format!("{PART_OF}type A {{\n  x Int\n}}\n");
    let b = format!("{PART_OF}model B {{\n  id Int @id\n  t Missing\n}}\n");
    let mut store = GraphStore::default();

    let first = refresh(
        &mut store,
        ROOT,
        &[(ROOT, both.as_str()), (A, a.as_str()), (B, b.as_str())],
    );
    assert_eq!(shown(&first, B).map(<[_]>::len), Some(1));

    let second = refresh(
        &mut store,
        ROOT,
        &[(ROOT, only_a.as_str()), (A, a.as_str()), (B, b.as_str())],
    );
    assert_eq!(shown(&second, B), Some(&[][..]), "b left the graph");
    let members = store.members_of(&uri(ROOT)).expect("root recorded");
    assert!(!members.contains(&uri(B)));
}

#[test]
fn a_shared_files_diagnostics_are_listed_once_across_graphs() {
    let importer = root("import \"money.cstack\"\n");
    let money = "type Money {\n  x Nope\n}\n";
    let files = [
        (R1, importer.as_str()),
        (R2, importer.as_str()),
        (MONEY, money),
    ];
    let mut store = GraphStore::default();
    refresh(&mut store, R1, &files);
    let second = refresh(&mut store, R2, &files);
    let in_money = shown(&second, MONEY).expect("money is published to");
    assert_eq!(in_money.len(), 1, "{in_money:?}");
}
