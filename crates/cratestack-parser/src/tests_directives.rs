//! `part` / `part of` / `import` parsing and `parse_file_at`
//! (docs/design/multi-file-schemas.md sections 2, 9).

use crate::parse::{ParsedFile, parse_file_at};

fn parse(source: &str) -> ParsedFile {
    parse_file_at(source, 0).expect("source parses")
}

fn error(source: &str) -> String {
    parse_file_at(source, 0)
        .err()
        .expect("source is refused")
        .message()
        .to_owned()
}

#[test]
fn part_directive_records_path_and_both_spans() {
    let source = "  part \"models/post.cstack\"\n";
    let directive = &parse(source).directives.parts[0];
    assert_eq!(directive.path, "models/post.cstack");
    assert_eq!(
        &source[directive.path_span.start..directive.path_span.end],
        "models/post.cstack"
    );
    assert_eq!(
        &source[directive.span.start..directive.span.end],
        "part \"models/post.cstack\""
    );
    assert_eq!((directive.span.line, directive.path_span.line), (1, 1));
}

#[test]
fn part_of_directive_is_matched_before_part() {
    let source = "part of \"../schema.cstack\"\n";
    let parsed = parse(source);
    let directive = parsed.directives.part_of.as_ref().unwrap();
    assert_eq!(directive.path, "../schema.cstack");
    assert_eq!(
        &source[directive.path_span.start..directive.path_span.end],
        "../schema.cstack"
    );
    assert!(parsed.directives.parts.is_empty());
}

#[test]
fn import_directive_records_path() {
    let source = "// shared\nimport \"../shared/money.cstack\"\n";
    let directive = &parse(source).directives.imports[0];
    assert_eq!(directive.path, "../shared/money.cstack");
    assert_eq!(directive.span.line, 2);
    assert_eq!(
        &source[directive.path_span.start..directive.path_span.end],
        "../shared/money.cstack"
    );
}

#[test]
fn several_parts_keep_source_order() {
    let parsed =
        parse("part \"b.cstack\"\npart \"a.cstack\"\nimport \"z.cstack\"\npart \"c.cstack\"\n");
    let paths: Vec<_> = parsed
        .directives
        .parts
        .iter()
        .map(|d| d.path.as_str())
        .collect();
    assert_eq!(paths, ["b.cstack", "a.cstack", "c.cstack"]);
    assert!(!parsed.directives.is_empty());
    assert!(parse("model A {\n  id Int @id\n}\n").directives.is_empty());
}

#[test]
fn a_transport_line_records_its_span_and_directives_stay_out_of_the_schema() {
    let source = "transport rpc\npart \"a.cstack\"\n";
    let parsed = parse(source);
    let span = parsed.directives.transport_span().unwrap();
    assert_eq!(&source[span.start..span.end], "transport rpc");
    assert!(parsed.schema.models.is_empty());
}

#[test]
fn duplicate_part_of_names_the_first_line() {
    let message = error("part of \"a.cstack\"\n\npart of \"b.cstack\"\n");
    assert_eq!(message, "duplicate `part of` (first declared on line 1)");
}

#[test]
fn directive_without_quoted_path_is_m16() {
    let cases = [
        ("part", "part"),
        ("part models/post.cstack", "part"),
        ("part \"unterminated", "part"),
        ("import", "import"),
        ("part of", "part of"),
        ("part of x", "part of"),
        ("part \"a.cstack\" trailing", "part"),
        ("import \"a.cstack\" // comment", "import"),
    ];
    for (source, keyword) in cases {
        assert_eq!(
            error(source),
            format!(
                "expected a quoted path after `{keyword}`, like `{keyword} \"models/post.cstack\"`"
            ),
            "source: {source}"
        );
    }
}

#[test]
fn words_that_only_start_with_a_directive_keyword_stay_unsupported() {
    assert_eq!(
        error("partial thing"),
        "unsupported top-level declaration: partial thing"
    );
    assert_eq!(
        error("imported x"),
        "unsupported top-level declaration: imported x"
    );
}
