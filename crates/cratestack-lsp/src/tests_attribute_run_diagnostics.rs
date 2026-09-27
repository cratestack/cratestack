//! Where the GHSA-69g4-xvcm-vm2j diagnostics land in an editor: on the
//! attribute the author has to change — never on its trailing comment,
//! its indentation, or a neighbour sharing its line — with LF and CRLF
//! line endings alike (a multi-line SQL body is joined with `\n`, so a
//! CRLF file is where a byte-count slip would show).

use std::str::FromStr;

use tower_lsp_server::ls_types::{Position, Range, Uri};

use crate::analyze::analyze_document;

const HEADER: &str = "datasource db {\n  provider = \"postgresql\"\n  url = env(\"DATABASE_URL\")\n}\n\nauth User {\n  id Int\n}\n\ntype R {\n  total Int\n}\n\n";
/// Lines in [`HEADER`], so a case can name its own line numbers.
const HEADER_LINES: u32 = 13;

/// Analyses `HEADER + body` with each newline style and asserts exactly one
/// diagnostic, on `line` (relative to `body`) from `column`, `len` characters
/// long, whose message contains `needle`.
fn assert_selects(body: &str, line: u32, column: u32, len: u32, needle: &str) {
    let Ok(uri) = Uri::from_str("file:///schema.cstack") else {
        unreachable!("the fixture URI is a literal")
    };
    for newline in ["\n", "\r\n"] {
        let text = format!("{HEADER}{body}").replace('\n', newline);
        let (_schema, diagnostics) = analyze_document(&uri, &text);
        assert_eq!(diagnostics.len(), 1, "{newline:?}: {diagnostics:?}");
        let line = HEADER_LINES + line;
        assert_eq!(
            diagnostics[0].range,
            Range {
                start: Position::new(line, column),
                end: Position::new(line, column + len),
            },
            "{newline:?}: {}",
            diagnostics[0].message
        );
        assert!(
            diagnostics[0].message.contains(needle),
            "{newline:?}: {}",
            diagnostics[0].message
        );
    }
}

#[test]
fn a_refused_procedure_attribute_is_selected_without_its_comment() {
    let body = "procedure p(): R\n  @allow(true)\n  @Deny(false) // note\n";
    assert_selects(
        body,
        2,
        2,
        "@Deny(false)".len() as u32,
        "unsupported attribute `@Deny`",
    );
}

#[test]
fn a_refused_attribute_sharing_a_line_is_selected_alone() {
    let body = "mutation procedure p(): R\n  @allow(true)\n  @no_idempotency @Deny(false)\n";
    let column = "  @no_idempotency ".len() as u32;
    assert_selects(body, 2, column, "@Deny(false)".len() as u32, "`@Deny`");
}

#[test]
fn a_detached_attribute_is_selected_without_indentation_or_comment() {
    let body = "procedure p(): R\n  @allow(true)\n\n  @deny(false) // note\n";
    assert_selects(
        body,
        3,
        2,
        "@deny(false)".len() as u32,
        "not directly under a signature",
    );
}

#[test]
fn a_run_into_the_next_declaration_selects_its_last_attribute() {
    let body = "procedure p(): R\n  @allow(true) // note\nprocedure q(): R\n  @allow(true)\n";
    assert_selects(
        body,
        1,
        2,
        "@allow(true)".len() as u32,
        "starts on the very next line",
    );
}

#[test]
fn an_attribute_after_a_multi_line_sql_body_is_selected_on_its_own_line() {
    let body = "query t(userId: Int): R\n  @@sql(\"\"\"\n    SELECT $1::bigint AS total\n  \"\"\") @Deny(false)\n";
    let column = "  \"\"\") ".len() as u32;
    assert_selects(body, 3, column, "@Deny(false)".len() as u32, "`@Deny`");
}

/// A view's block attribute keeps the span `collect_attribute_text` gives
/// it (a query's is recomputed per attribute), so this is the case that
/// pins that function's end offset.
#[test]
fn a_multi_line_view_sql_attribute_ends_on_its_closing_line() {
    let Ok(uri) = Uri::from_str("file:///schema.cstack") else {
        unreachable!("the fixture URI is a literal")
    };
    let body = "model Post {\n  id Int @id\n}\n\nview V from Post {\n  id Int @id\n  @@server_sql(\"\"\"\n    SELECT id FROM post\n  \"\"\") @@paged // note\n}\n";
    for newline in ["\n", "\r\n"] {
        let text = format!("{HEADER}{body}").replace('\n', newline);
        let (_schema, diagnostics) = analyze_document(&uri, &text);
        assert_eq!(diagnostics.len(), 1, "{newline:?}: {diagnostics:?}");
        assert_eq!(
            diagnostics[0].range,
            Range {
                start: Position::new(HEADER_LINES + 6, 2),
                end: Position::new(HEADER_LINES + 8, "  \"\"\") @@paged".len() as u32),
            },
            "{newline:?}: {}",
            diagnostics[0].message
        );
    }
}
