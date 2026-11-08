//! `SchemaError` related locations and their rendering
//! (docs/design/multi-file-schemas.md section 9).

use std::sync::Arc;

use cratestack_core::SourceSpan;

use crate::diagnostics::SchemaError;

fn strip_ansi(text: &str) -> String {
    let mut out = String::new();
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

#[test]
fn an_error_with_a_related_span_in_a_second_file_renders_both_frames() {
    let here: Arc<str> = Arc::from("models/a.cstack");
    let here_text: Arc<str> = Arc::from("model Dup {\n  id Int @id\n}\n");
    let mut error = SchemaError::new("`Dup` is declared twice", 6..9, 1).with_related(
        "first declared here",
        SourceSpan {
            start: 6,
            end: 9,
            line: 1,
        },
    );
    // The loader attributes the related span to its own file; here both
    // start empty, so tag the primary and then retarget the related span.
    error = error.with_file(&here, &here_text);
    let there: Arc<str> = Arc::from("models/b.cstack");
    let there_text: Arc<str> = Arc::from("// earlier\nmodel Dup {\n  id Int @id\n}\n");
    error.related[0].file = Arc::clone(&there);
    error.related[0].source_text = Arc::clone(&there_text);
    error.related[0].span = 17..20;
    error.related[0].line = 2;

    assert_eq!(error.related()[0].file(), "models/b.cstack");
    assert_eq!(error.related()[0].message(), "first declared here");
    assert_eq!(error.related()[0].span(), 17..20);
    assert_eq!(error.related()[0].line(), 2);

    let rendered = strip_ansi(&error.render());
    assert!(rendered.contains("models/a.cstack"), "{rendered}");
    assert!(rendered.contains("models/b.cstack"), "{rendered}");
    assert!(rendered.contains("`Dup` is declared twice"), "{rendered}");
    assert!(rendered.contains("first declared here"), "{rendered}");
    assert_eq!(rendered.matches("model Dup").count(), 2, "{rendered}");
    assert!(rendered.contains("models/b.cstack:2:7"), "{rendered}");
}

#[test]
fn with_file_tags_a_related_span_that_has_no_file_yet() {
    let file: Arc<str> = Arc::from("schema.cstack");
    let text: Arc<str> = Arc::from("model A {\n}\n");
    let error = SchemaError::new("boom", 0..5, 1)
        .with_related(
            "here too",
            SourceSpan {
                start: 6,
                end: 7,
                line: 1,
            },
        )
        .with_file(&file, &text);
    assert_eq!(error.related()[0].file(), "schema.cstack");
    let rendered = strip_ansi(&error.render());
    assert!(rendered.contains("here too"), "{rendered}");
}

/// Captured from the renderer before related locations existed: an error with
/// none must keep rendering byte-for-byte as it did.
#[test]
fn an_error_without_related_spans_renders_exactly_as_before() {
    let error =
        crate::parse_schema_named("schema.cstack", "model A {\n  id Int @id\n  x Missing\n}\n")
            .unwrap_err();
    assert!(error.related().is_empty());
    assert_eq!(error.render(), EXPECTED_PLAIN);
}

const EXPECTED_PLAIN: &str = "\u{1b}[31mError:\u{1b}[0m unknown type `Missing`\n   \u{1b}[38;5;246m╭\u{1b}[0m\u{1b}[38;5;246m─\u{1b}[0m\u{1b}[38;5;246m[\u{1b}[0m schema.cstack:3:1 \u{1b}[38;5;246m]\u{1b}[0m\n   \u{1b}[38;5;246m│\u{1b}[0m\n \u{1b}[38;5;246m3 │\u{1b}[0m \u{1b}[31m \u{1b}[0m\u{1b}[31m \u{1b}[0m\u{1b}[31mx\u{1b}[0m\u{1b}[31m \u{1b}[0m\u{1b}[31mM\u{1b}[0m\u{1b}[31mi\u{1b}[0m\u{1b}[31ms\u{1b}[0m\u{1b}[31ms\u{1b}[0m\u{1b}[31mi\u{1b}[0m\u{1b}[31mn\u{1b}[0m\u{1b}[31mg\u{1b}[0m\n \u{1b}[38;5;240m  │\u{1b}[0m \u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m┬\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m  \n \u{1b}[38;5;240m  │\u{1b}[0m      \u{1b}[31m╰\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m\u{1b}[31m─\u{1b}[0m unknown type `Missing`\n\u{1b}[38;5;246m───╯\u{1b}[0m\n";
