//! `import` refusals: M10, M11, M12 and M13 with an import
//! (docs/design/multi-file-schemas.md section 9).

mod multi_file_support;

use multi_file_support::{HEAD, ROOT, assert_primary, assert_related, load};

const SHARED: &str = "/fixture/shared.cstack";

fn root(body: &str) -> String {
    format!("{HEAD}{body}")
}

#[test]
fn importing_a_part_is_m10() {
    let root = root("import \"p.cstack\"\n");
    let p = "part of \"schema.cstack\"\n\ntype T {\n  x Int\n}\n";
    let files = [(ROOT, root.as_str()), ("/fixture/p.cstack", p)];
    let error = load(&files).unwrap_err();
    assert_primary(
        &files,
        &error,
        "cannot import `p.cstack`: it is a part of `schema.cstack`, and a part is locked to \
         its root; import a file that does not declare `part of` instead",
        ROOT,
        "import \"p.cstack\"",
    );
    assert_related(
        &files,
        &error,
        "/fixture/p.cstack",
        "part of \"schema.cstack\"",
        "`p.cstack` declares `part of` here",
    );
}

#[test]
fn a_cycle_is_m11_with_the_full_path() {
    let root = root("import \"shared/a.cstack\"\n");
    let a = "import \"b.cstack\"\n";
    let b = "import \"a.cstack\"\n";
    let files = [
        (ROOT, root.as_str()),
        ("/fixture/shared/a.cstack", a),
        ("/fixture/shared/b.cstack", b),
    ];
    let error = load(&files).unwrap_err();
    assert_primary(
        &files,
        &error,
        "import cycle: `schema.cstack` → `shared/a.cstack` → `shared/b.cstack` → \
         `shared/a.cstack`",
        "/fixture/shared/b.cstack",
        "import \"a.cstack\"",
    );
}

#[test]
fn a_self_import_is_m11() {
    let root = root("import \"schema.cstack\"\n");
    let files = [(ROOT, root.as_str())];
    let error = load(&files).unwrap_err();
    assert_primary(
        &files,
        &error,
        "import cycle: `schema.cstack` → `schema.cstack`",
        ROOT,
        "import \"schema.cstack\"",
    );
}

#[test]
fn a_part_importing_its_own_root_is_m11() {
    let root = root("part \"models/post.cstack\"\n");
    let post = "part of \"../schema.cstack\"\nimport \"../schema.cstack\"\n";
    let files = [(ROOT, root.as_str()), ("/fixture/models/post.cstack", post)];
    let error = load(&files).unwrap_err();
    assert_primary(
        &files,
        &error,
        "import cycle: `schema.cstack` (part `models/post.cstack`) → `schema.cstack`",
        "/fixture/models/post.cstack",
        "import \"../schema.cstack\"",
    );
}

/// (declaration, how the message names it, what the span starts with)
const FORBIDDEN: &[(&str, &str, &str)] = &[
    ("model M {\n  id Int @id\n}\n", "model M", "model M {"),
    (
        "view V from M {\n  id Int @id\n}\n",
        "view V",
        "view V from M {",
    ),
    (
        "procedure p(args: T): Int\n  @allow(true)\n",
        "procedure p",
        "procedure p(args: T): Int",
    ),
    (
        "mutation procedure q(args: T): Int\n  @allow(true)\n",
        "mutation procedure q",
        "mutation procedure q(args: T): Int",
    ),
    (
        "subscription procedure s(args: T): T\n",
        "subscription procedure s",
        "subscription procedure s(args: T): T",
    ),
    (
        "topic Tp(roomId: String): T\n",
        "topic Tp",
        "topic Tp(roomId: String): T",
    ),
    (
        "query qq(userId: Int): T\n  @@sql(\"SELECT 1\")\n  @allow(true)\n",
        "query qq",
        "query qq(userId: Int): T",
    ),
    (
        "datasource other {\n  provider = \"postgresql\"\n}\n",
        "datasource other",
        "datasource other {",
    ),
    ("auth Who {\n  id Int\n}\n", "auth Who", "auth Who {"),
    ("transport rpc\n", "transport", "transport rpc"),
    ("mcp {\n  expose = [tools]\n}\n", "mcp", "mcp {"),
    (
        "extension pgvector {\n}\n",
        "extension pgvector",
        "extension pgvector {",
    ),
];

fn expect_m12(files: &[(&str, &str)], what: &str, span_text: &str, file: &str, name: &str) {
    let error = load(files).unwrap_err();
    assert_eq!(
        error.message(),
        format!(
            "`{what}` cannot come from an import: `{name}` is imported by `schema.cstack`, and \
             an imported file may declare only `type`, `enum` and `mixin`; use `part` to split \
             one schema across files"
        )
    );
    assert_eq!(error.file(), file);
    let covered = multi_file_support::primary_text(files, &error);
    assert!(covered.starts_with(span_text), "{covered:?}");
    assert_related(
        files,
        &error,
        ROOT,
        "import \"shared.cstack\"",
        &format!("`{name}` is imported here"),
    );
}

#[test]
fn each_disallowed_declaration_in_an_import_is_m12() {
    let root = root("import \"shared.cstack\"\n");
    for (declaration, what, span_text) in FORBIDDEN {
        let text = format!("type T {{\n  x Int\n}}\n\n{declaration}");
        let files = [(ROOT, root.as_str()), (SHARED, text.as_str())];
        expect_m12(&files, what, span_text, SHARED, "shared.cstack");
    }
}

#[test]
fn each_disallowed_declaration_in_an_imported_roots_part_is_m12() {
    let root = root("import \"shared.cstack\"\n");
    let shared = "part \"shared/p.cstack\"\n\ntype T {\n  x Int\n}\n";
    for (declaration, what, span_text) in FORBIDDEN {
        let part = format!("part of \"../shared.cstack\"\n\n{declaration}");
        let files = [
            (ROOT, root.as_str()),
            (SHARED, shared),
            ("/fixture/shared/p.cstack", part.as_str()),
        ];
        expect_m12(
            &files,
            what,
            span_text,
            "/fixture/shared/p.cstack",
            "shared/p.cstack",
        );
    }
}

#[test]
fn an_import_colliding_with_the_importer_is_m13() {
    let root = root("import \"shared.cstack\"\n\ntype Money {\n  x Int\n}\n");
    let shared = "type Money {\n  y Int\n}\n";
    let files = [(ROOT, root.as_str()), (SHARED, shared)];
    let error = load(&files).unwrap_err();
    assert_primary(
        &files,
        &error,
        "`Money` is declared twice: `type Money` in `schema.cstack` (line 7) and `type Money` \
         in `shared.cstack` (line 1)",
        SHARED,
        "Money",
    );
}

#[test]
fn an_unreadable_import_is_m14() {
    let root = root("import \"missing.cstack\"\n");
    let files = [(ROOT, root.as_str())];
    let error = load(&files).unwrap_err();
    assert!(
        error
            .message()
            .starts_with("cannot read `missing.cstack` (listed on line 5 of `schema.cstack`): "),
        "{}",
        error.message()
    );
}
