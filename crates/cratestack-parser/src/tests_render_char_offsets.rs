//! Code frames keep their line and column after non-ASCII text (cratestack#1157).
//!
//! `SchemaError` spans are byte offsets into the source, but ariadne counts
//! characters by default. Before the fix `render()` handed ariadne the byte
//! offsets unchanged, so one multi-byte character before an error made the
//! frame lose its location, or point at the wrong line in a long file.
//!
//! Every case here puts the error on line 28 and at column 28. The field
//! attribute `@raedonly` starts 27 characters into its line (the same
//! character count for every value below, because the value is three
//! characters whatever their UTF-8 width). A byte-based column would read 34
//! for the `é✓𝄞` value, so the column assertion catches a same-line miscount.

use crate::parse_schema_diagnostics;

const FILE: &str = "schema.cstack";
const ERROR_LINE: &str = "  x String @default(\"{VALUE}\") @raedonly";

/// Remove ANSI SGR escapes so the rendered frame can be matched as plain text.
///
/// ariadne wraps each character of a code frame in its own escape pair, so a
/// `contains` against the raw output would never match a source line.
fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            for escape in chars.by_ref() {
                if escape.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(ch);
        }
    }
    out
}

/// A schema whose 20 comment lines (lines 6 to 25) each carry `comment`, then
/// a model whose error is on line 28. `value` is the `@default` string on
/// that same line, so it sits before the error on that line too.
fn schema(comment: &str, value: &str) -> String {
    let mut source = String::from(
        "datasource db {\n  provider = \"postgresql\"\n  url = env(\"DATABASE_URL\")\n}\n\n",
    );
    for n in 1..=20 {
        source.push_str(&format!("// {comment} {n}\n"));
    }
    source.push_str("model A {\n  id Int @id\n");
    source.push_str(&ERROR_LINE.replace("{VALUE}", value));
    source.push_str("\n}\n");
    source
}

/// The rendered frame of the first error `source` produces.
fn rendered_first_error(source: &str) -> String {
    let (_, errors) = parse_schema_diagnostics(FILE, source);
    let error = errors.first().expect("the schema must fail to parse");
    strip_ansi(&error.render())
}

/// Assert the frame names the error's `line:col` and quotes its source line.
fn assert_frame_points_at_error(source: &str, value: &str) {
    let plain = rendered_first_error(source);
    let error_line = ERROR_LINE.replace("{VALUE}", value);

    assert!(
        plain.contains(&format!("{FILE}:28:28")),
        "expected the location {FILE}:28:28 in the frame, got:\n{plain}"
    );
    assert!(
        plain.contains(&error_line),
        "expected the offending source line in the frame, got:\n{plain}"
    );
}

/// ASCII control: the same layout with no multi-byte text. This passes before
/// and after the fix; it pins that the fix leaves ASCII frames unchanged.
#[test]
fn ascii_only_schema_reports_line_and_column() {
    assert_frame_points_at_error(&schema("comment line", "abc"), "abc");
}

/// 2-byte UTF-8 (`é`) on every earlier line and in the error line's value.
#[test]
fn two_byte_utf8_before_error_keeps_line_and_column() {
    assert_frame_points_at_error(&schema("commentaire é", "ééé"), "ééé");
}

/// 3-byte UTF-8 (`✓`) on every earlier line and in the error line's value.
#[test]
fn three_byte_utf8_before_error_keeps_line_and_column() {
    assert_frame_points_at_error(&schema("commentaire ✓", "✓✓✓"), "✓✓✓");
}

/// 4-byte UTF-8 (`𝄞`, `😀`) on every earlier line and in the error line's value.
#[test]
fn four_byte_utf8_before_error_keeps_line_and_column() {
    assert_frame_points_at_error(&schema("musique 𝄞 😀", "𝄞𝄞𝄞"), "𝄞𝄞𝄞");
}

/// A BEL (one byte) before the error. `substitute_for_display` draws it as
/// `␇` (three bytes), so the frame's bytes no longer match the source's. The
/// span must still resolve against the source, not against the frame.
#[test]
fn control_character_before_error_keeps_line_and_column() {
    assert_frame_points_at_error(&schema("comment \u{7}", "abc"), "abc");
}

/// All three widths mixed, the shape the issue reports: `é`, `✓` and `𝄞` in
/// one value, the value sitting on the error line before the attribute.
#[test]
fn mixed_utf8_widths_before_error_keep_line_and_column() {
    assert_frame_points_at_error(&schema("commentaire éèàçù ✓ 𝄞", "é✓𝄞"), "é✓𝄞");
}
