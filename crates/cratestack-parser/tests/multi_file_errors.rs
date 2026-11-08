//! M13: one name declared in two files, and the invariant that splitting a
//! schema never turns a valid one invalid (docs/design/multi-file-schemas.md
//! section 9).

mod multi_file_support;

use cratestack_parser::parse_schema;
use multi_file_support::{HEAD, ROOT, assert_primary, assert_related, load};

const A: &str = "/fixture/a.cstack";
const B: &str = "/fixture/b.cstack";
const PART_OF: &str = "part of \"schema.cstack\"\n\n";

fn root(listing: &str) -> String {
    format!("{HEAD}{listing}")
}

#[test]
fn a_duplicate_model_across_parts_is_m13() {
    let (a, b) = (
        format!("{PART_OF}model Foo {{\n  id Int @id\n}}\n"),
        format!("{PART_OF}\n\nmodel Foo {{\n  id Int @id\n}}\n"),
    );
    let root = root("part \"a.cstack\"\npart \"b.cstack\"\n");
    let files = [(ROOT, root.as_str()), (A, a.as_str()), (B, b.as_str())];
    let error = load(&files).unwrap_err();
    assert_primary(
        &files,
        &error,
        "`Foo` is declared twice: `model Foo` in `a.cstack` (line 3) and `model Foo` in \
         `b.cstack` (line 5)",
        B,
        "Foo",
    );
    assert_eq!(error.line(), 5);
    assert_related(
        &files,
        &error,
        A,
        "Foo",
        "`model Foo` is declared here first",
    );
    assert_eq!(error.related()[0].line(), 3);
}

#[test]
fn a_model_against_a_type_of_the_same_name_is_m13() {
    let a = format!("{PART_OF}type Foo {{\n  x Int\n}}\n");
    let b = format!("{PART_OF}model Foo {{\n  id Int @id\n}}\n");
    let root = root("part \"a.cstack\"\npart \"b.cstack\"\n");
    let files = [(ROOT, root.as_str()), (A, a.as_str()), (B, b.as_str())];
    let error = load(&files).unwrap_err();
    assert_primary(
        &files,
        &error,
        "`Foo` is declared twice: `type Foo` in `a.cstack` (line 3) and `model Foo` in \
         `b.cstack` (line 3)",
        B,
        "Foo",
    );
    assert_related(
        &files,
        &error,
        A,
        "Foo",
        "`type Foo` is declared here first",
    );
}

#[test]
fn a_duplicate_procedure_across_parts_is_m13() {
    let proc = "procedure ping(args: Arg): Arg\n  @allow(true)\n";
    let a = format!("{PART_OF}type Arg {{\n  x Int\n}}\n\n{proc}");
    let b = format!("{PART_OF}mutation {proc}");
    let root = root("part \"a.cstack\"\npart \"b.cstack\"\n");
    let files = [(ROOT, root.as_str()), (A, a.as_str()), (B, b.as_str())];
    let error = load(&files).unwrap_err();
    assert_primary(
        &files,
        &error,
        "`ping` is declared twice: `procedure ping` in `a.cstack` (line 7) and `mutation \
         procedure ping` in `b.cstack` (line 3)",
        B,
        "ping",
    );
    assert_related(
        &files,
        &error,
        A,
        "ping",
        "`procedure ping` is declared here first",
    );
}

/// Each declaration, named `Foo`, valid beside the shared base.
const BASE: &str = "type Arg {\n  x Int\n}\n\nmodel Base {\n  id Int @id\n}\n\ntype Line {\n  \
    body String\n}\n\n";

fn declarations() -> Vec<(&'static str, &'static str, &'static str)> {
    // (label, bucket, text)
    vec![
        ("type", "type", "type Foo {\n  x Int\n}\n"),
        ("enum", "type", "enum Foo {\n  A\n}\n"),
        ("model", "type", "model Foo {\n  id Int @id\n}\n"),
        ("mixin", "type", "mixin Foo {\n  x Int\n}\n"),
        (
            "procedure",
            "procedure",
            "procedure Foo(args: Arg): Arg\n  @allow(true)\n",
        ),
        (
            "mutation procedure",
            "procedure",
            "mutation procedure Foo(args: Arg): Arg\n  @allow(true)\n",
        ),
        (
            "subscription procedure",
            "procedure",
            "subscription procedure Foo(args: Arg): Line\n  @allow(true)\n",
        ),
        (
            "query",
            "query",
            "query Foo(userId: String): Arg\n  @@sql(\"\"\"\n    SELECT 1 AS \"x\" WHERE $1 = $1\n  \
             \"\"\")\n  @allow(true)\n",
        ),
        ("topic", "topic", "topic Foo(roomId: String): Line\n"),
        (
            "view",
            "view",
            "view Foo from Base {\n  id Int @id @from(Base.id)\n\n  @@server_sql(\"SELECT id FROM \
             base\")\n  @@embedded_sql(\"SELECT id FROM base\")\n  @@allow(\"read\", true)\n}\n",
        ),
    ]
}

#[test]
fn cross_file_collisions_are_refused_only_where_one_file_would_be() {
    let decls = declarations();
    let root_text = root("part \"a.cstack\"\npart \"b.cstack\"\n");
    // The base alone, and each declaration with it, is a valid schema.
    for (label, _, text) in &decls {
        let alone = format!("{HEAD}{BASE}{text}");
        parse_schema(&alone).unwrap_or_else(|e| panic!("{label} alone should be valid: {e}"));
    }
    let (mut refused, mut accepted) = (0, 0);
    for (label_a, bucket_a, text_a) in &decls {
        for (label_b, bucket_b, text_b) in &decls {
            let a = format!("{PART_OF}{BASE}{text_a}");
            let b = format!("{PART_OF}{text_b}");
            // The base is declared once: in `a`. `b` borrows its names.
            let files = [(ROOT, root_text.as_str()), (A, a.as_str()), (B, b.as_str())];
            let split = load(&files);
            let single = parse_schema(&format!("{HEAD}{BASE}{text_a}\n{text_b}"));
            assert_eq!(
                split.is_err(),
                single.is_err(),
                "{label_a} then {label_b}: split {:?}, concatenated {:?}",
                split.as_ref().err().map(|e| e.message().to_owned()),
                single.as_ref().err().map(|e| e.message().to_owned()),
            );
            if let Err(error) = &split {
                let is_m13 = error.message().contains("is declared twice");
                assert_eq!(is_m13, bucket_a == bucket_b, "{label_a} then {label_b}");
                refused += 1;
            } else {
                accepted += 1;
            }
        }
    }
    assert!(
        refused >= 10 && accepted >= 10,
        "refused {refused}, accepted {accepted}"
    );
}
