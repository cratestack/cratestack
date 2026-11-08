//! `analyze_graph`: diagnostics attributed to the file they belong to
//! (cratestack#921).

use std::path::Path;

use tower_lsp_server::ls_types::{Position, Range};

use crate::analyze::analyze_document;
use crate::tests_graph_support::{A, B, PART_OF, ROOT, analyze, diagnostics_of, root, uri};

fn range(start: (u32, u32), end: (u32, u32)) -> Range {
    Range {
        start: Position::new(start.0, start.1),
        end: Position::new(end.0, end.1),
    }
}

#[test]
fn a_single_file_graph_reports_what_analyze_document_reports() {
    let fixtures = [
        "model User {\n  id Int @id\n}\n",
        "mode User {\n  id Int @id\n}\n",
        "model User {\n  id Int @id\n}\nmodel User {\n  id Int @id\n}\n",
        "model Post {\n  id Int @id\n  owner Missing\n}\ntype T {\n  x Absent\n}\n",
        "model Post {\n  id Int @id\n  authorId Int\n  author Post @relation(fields:[nope],references:[id])\n}\n",
        "widget Nope\n",
    ];
    for text in fixtures {
        let (_, expected) = analyze_document(&uri("/solo/schema.cstack"), text);
        let analysis = analyze("/solo/schema.cstack", &[("/solo/schema.cstack", text)]);
        assert_eq!(
            diagnostics_of(&analysis, "/solo/schema.cstack"),
            expected.as_slice(),
            "fixture:\n{text}"
        );
        assert_eq!(analysis.members.len(), 1);
        assert_eq!(analysis.view.is_some(), expected.is_empty(), "{text}");
    }
}

#[test]
fn diagnostics_are_published_to_the_file_they_belong_to() {
    let root = root("part \"a.cstack\"\npart \"b.cstack\"\n");
    let a = format!("{PART_OF}model A {{\n  id Int @id\n  owner Missing\n}}\n");
    let b = format!("{PART_OF}type B {{\n  x Int\n}}\n");
    let files = [(ROOT, root.as_str()), (A, a.as_str()), (B, b.as_str())];
    let analysis = analyze(ROOT, &files);

    assert!(analysis.view.is_none());
    let in_a = diagnostics_of(&analysis, A);
    assert_eq!(in_a.len(), 1, "{in_a:?}");
    assert_eq!(in_a[0].message, "unknown type `Missing`");
    // `  owner Missing` is line 4 (0-based 4) of a.cstack, where the range is
    // in a.cstack's own text, not the merged address space.
    assert_eq!(in_a[0].range.start.line, 4);
    assert!(diagnostics_of(&analysis, B).is_empty());
    assert!(diagnostics_of(&analysis, ROOT).is_empty());
    assert_eq!(analysis.members.len(), 3, "every file gets an entry");
    assert_eq!(analysis.root, uri(ROOT));
}

#[test]
fn a_part_opened_alone_is_checked_through_its_root() {
    let root = root("part \"a.cstack\"\npart \"b.cstack\"\n");
    let a = format!("{PART_OF}model A {{\n  id Int @id\n  b B\n}}\n");
    let b = format!("{PART_OF}enum B {{\n  X\n}}\n");
    let files = [(ROOT, root.as_str()), (A, a.as_str()), (B, b.as_str())];

    // Clean: the real schema loads, not "missing datasource" on a bare part.
    let clean = analyze(A, &files);
    assert!(clean.view.is_some());
    assert!(clean.diagnostics.values().all(Vec::is_empty));
    assert_eq!(clean.root, uri(ROOT));

    let broken_a = format!("{PART_OF}model A {{\n  id Int @id\n  b Nope\n}}\n");
    let files = [
        (ROOT, root.as_str()),
        (A, broken_a.as_str()),
        (B, b.as_str()),
    ];
    let alone = analyze(A, &files);
    let in_a = diagnostics_of(&alone, A);
    assert_eq!(in_a.len(), 1, "{in_a:?}");
    assert_eq!(in_a[0].message, "unknown type `Nope`");
}

#[test]
fn a_part_whose_root_does_not_list_it_reports_m3() {
    let root = root("part \"b.cstack\"\n");
    let a = format!("{PART_OF}model A {{\n  id Int @id\n}}\n");
    let b = format!("{PART_OF}type B {{\n  x Int\n}}\n");
    let files = [(ROOT, root.as_str()), (A, a.as_str()), (B, b.as_str())];
    let analysis = analyze(A, &files);
    let in_a = diagnostics_of(&analysis, A);
    assert_eq!(in_a.len(), 1, "{in_a:?}");
    assert_eq!(
        in_a[0].message,
        "`a.cstack` declares itself a part of `schema.cstack`, but `schema.cstack` does not \
         list it; add `part \"a.cstack\"` to `schema.cstack`"
    );
    // On the `part of` line.
    assert_eq!(in_a[0].range.start.line, 0);
}

#[test]
fn a_part_whose_root_is_missing_reports_the_unreadable_root_on_the_part() {
    let a = format!("{PART_OF}model A {{\n  id Int @id\n}}\n");
    let analysis = analyze(A, &[(A, a.as_str())]);
    let in_a = diagnostics_of(&analysis, A);
    assert_eq!(in_a.len(), 1, "{in_a:?}");
    assert!(
        in_a[0]
            .message
            .starts_with("failed to read schema file /fixture/schema.cstack"),
        "{}",
        in_a[0].message
    );
    // The missing root is not a file to publish to.
    assert_eq!(analysis.members.len(), 1);
    assert_eq!(analysis.root, uri(A));
    assert_eq!(in_a[0].range, range((0, 0), (0, 0)));
}

#[test]
fn an_unreadable_listed_part_reports_m14_on_the_listing_line() {
    let root = root("part \"a.cstack\"\npart \"gone.cstack\"\n");
    let a = format!("{PART_OF}model A {{\n  id Int @id\n}}\n");
    let analysis = analyze(ROOT, &[(ROOT, root.as_str()), (A, a.as_str())]);
    let in_root = diagnostics_of(&analysis, ROOT);
    assert_eq!(in_root.len(), 1, "{in_root:?}");
    assert!(
        in_root[0].message.starts_with("cannot read `gone.cstack`"),
        "{}",
        in_root[0].message
    );
    assert_eq!(in_root[0].range.start.line, 10);
    assert!(Path::new(ROOT).is_absolute());
}

#[test]
fn a_cross_file_duplicate_carries_related_information() {
    let root = root("part \"a.cstack\"\npart \"b.cstack\"\n");
    let a = format!("{PART_OF}model Foo {{\n  id Int @id\n}}\n");
    let b = format!("{PART_OF}\nmodel Foo {{\n  id Int @id\n}}\n");
    let files = [(ROOT, root.as_str()), (A, a.as_str()), (B, b.as_str())];
    let analysis = analyze(ROOT, &files);

    let in_b = diagnostics_of(&analysis, B);
    assert_eq!(in_b.len(), 1, "{in_b:?}");
    assert!(in_b[0].message.starts_with("`Foo` is declared twice"));
    let related = in_b[0]
        .related_information
        .as_ref()
        .expect("a related location");
    assert_eq!(related.len(), 1);
    assert_eq!(related[0].location.uri, uri(A));
    assert_eq!(related[0].message, "`model Foo` is declared here first");
    assert_eq!(related[0].location.range, range((2, 6), (2, 9)));
    assert!(diagnostics_of(&analysis, A).is_empty());
}
