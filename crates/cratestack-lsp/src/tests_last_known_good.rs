//! Retention of the last schema that parsed.
//!
//! While someone types, a file spends most of its time syntactically invalid.
//! Dropping the schema on every failed parse made navigation, hover, symbols
//! and colouring flicker off and on with each keystroke; retaining it keeps
//! them steady. These tests pin the rule and, just as importantly, its limits.

use std::str::FromStr;

use tower_lsp_server::ls_types::Uri;

use crate::analyze::analyze_document;
use crate::definition::definition_location;
use crate::semantic_tokens::semantic_tokens;
use crate::state::{DocumentState, next_document_state};
use crate::text::word_at_offset;

const VALID: &str = "model User {\n  id Int @id\n}\n\nmodel Post {\n  id Int @id\n  authorId Int\n  author User @relation(fields:[authorId],references:[id])\n}\n";

/// The same schema mid-edit: `mode` is not a declaration keyword yet, so the
/// whole file fails to parse.
const BROKEN: &str = "mode User {\n  id Int @id\n}\n\nmodel Post {\n  id Int @id\n  authorId Int\n  author User @relation(fields:[authorId],references:[id])\n}\n";

fn uri() -> Uri {
    Uri::from_str("file:///schema.cstack").expect("uri should parse")
}

fn apply(previous: Option<DocumentState>, text: &str) -> DocumentState {
    let (schema, _) = analyze_document(&uri(), text);
    next_document_state(previous, text.to_owned(), schema)
}

#[test]
fn a_failed_parse_keeps_the_previous_schema_and_its_text() {
    let good = apply(None, VALID);
    assert!(good.resolved().is_some(), "fixture should parse");

    let broken = apply(Some(good), BROKEN);
    let (text, schema) = broken
        .resolved()
        .expect("the last good schema should survive a failed parse");

    assert_eq!(schema.models.len(), 2);
    assert_eq!(
        text, VALID,
        "the retained text must be the one the schema was parsed from, not the current buffer",
    );
    assert!(broken.is_stale());
}

/// The pairing is the correctness argument: spans index into the text that
/// produced them, so a stale lookup must land in exactly the same place a
/// fresh one does. Pairing the retained schema with the *current* buffer
/// instead shifts every offset past the edit and silently resolves to the
/// wrong position — which looks like working navigation, just wrong.
#[test]
fn stale_navigation_resolves_to_the_same_place_as_a_fresh_parse() {
    let fresh = apply(None, VALID);
    let (fresh_text, fresh_schema) = fresh.resolved().expect("fixture should parse");
    let fresh_offset = fresh_text
        .rfind("authorId")
        .expect("relation field should exist");
    let expected = definition_location(&uri(), fresh_text, fresh_schema, fresh_offset)
        .expect("definition should resolve");

    let stale = apply(Some(apply(None, VALID)), BROKEN);
    let (text, schema) = stale.resolved().expect("schema should survive");
    let offset = text.rfind("authorId").expect("relation field should exist");
    let actual =
        definition_location(&uri(), text, schema, offset).expect("definition should resolve");

    assert_eq!(word_at_offset(text, offset), Some("authorId"));
    assert_eq!(
        actual.range, expected.range,
        "a stale lookup must resolve to the same range as a live one",
    );
    assert_ne!(
        VALID.len(),
        BROKEN.len(),
        "the two texts must differ in length or this proves nothing",
    );
}

#[test]
fn semantic_tokens_survive_a_failed_parse() {
    let good = apply(None, VALID);
    let fresh = good.resolved().map(|(t, s)| semantic_tokens(t, s).len());

    let broken = apply(Some(good), BROKEN);
    let stale = broken.resolved().map(|(t, s)| semantic_tokens(t, s).len());

    assert_eq!(
        fresh, stale,
        "colouring should not go flat because the file is momentarily invalid",
    );
    assert!(fresh.expect("tokens should exist") > 0);
}

/// The fallback is a *fallback*, not a fiction: a file that has never parsed
/// has no schema to serve, and inventing one would be worse than silence.
#[test]
fn a_document_that_never_parsed_has_no_schema() {
    let broken = apply(None, BROKEN);

    assert!(broken.resolved().is_none());
    assert!(
        !broken.is_stale(),
        "nothing retained means nothing stale — `is_stale` must not report true for an empty slot",
    );
}

#[test]
fn a_successful_parse_replaces_the_retained_schema_and_clears_staleness() {
    let recovered = apply(Some(apply(Some(apply(None, VALID)), BROKEN)), VALID);
    let (text, schema) = recovered.resolved().expect("schema should be present");

    assert_eq!(text, VALID);
    assert_eq!(schema.models.len(), 2);
    assert!(
        !recovered.is_stale(),
        "once the buffer parses again, results are live",
    );
}

/// A stale hover must say so. The failure this guards against is silent: a
/// popup describing the file as it was several keystrokes ago, with nothing
/// telling the reader that is what they are looking at.
#[test]
fn hover_marks_stale_results_and_leaves_live_ones_alone() {
    use crate::hover::locate_symbol;
    use crate::hover_render::hover_markdown;

    let good = apply(None, VALID);
    let (text, schema) = good.resolved().expect("schema should parse");
    let offset = text.find("User").expect("model should exist");
    let symbol = locate_symbol(schema, offset).expect("symbol should resolve");

    let live = hover_markdown(&symbol, false);
    let stale = hover_markdown(&symbol, true);

    assert!(!live.contains("last version"));
    assert!(stale.contains("last version of this file that parsed"));
    assert!(
        stale.starts_with(&live),
        "the marker should be additive, never a replacement for the real content",
    );
}
