//! Rename coverage for the `query` block (cratestack#867).
//!
//! Split from [`super::tests_queries`] for the workspace's 200-line
//! ceiling, and a reasonable seam regardless: rename is the one request
//! here that *writes*, so — as `rename.rs`'s own module doc puts it — a
//! wrong result rewrites source and is easy to miss in a diff.
//!
//! Shares that module's fixture rather than restating it, so the two
//! cannot drift on what the schema under test actually says.

use std::str::FromStr;

use tower_lsp_server::ls_types::{Position, Uri};

use crate::analyze::analyze_document;
use crate::rename::rename_ranges;
use crate::state::{DocumentState, next_document_state};
use crate::tests_queries::{SCHEMA, offset_of};
use crate::text::{offset_to_position, position_to_offset};

fn document() -> DocumentState {
    let uri = Uri::from_str("file:///schema.cstack").expect("uri should parse");
    let (parsed, _) = analyze_document(&uri, SCHEMA);
    next_document_state(None, SCHEMA.to_owned(), parsed)
}

fn position_at(needle: &str, occurrence: usize) -> Position {
    offset_to_position(SCHEMA, offset_of(needle, occurrence))
}

/// Renaming the `type` a query returns must rewrite the query's signature
/// too, or the rename leaves the schema uncompilable.
///
/// `query_symbols::collect_type_reference_spans` exists for exactly this,
/// and its doc comment says so — but nothing tested it until
/// cratestack#870's review pointed that out. A doc comment claiming a
/// guarantee, with no test, is the shape of a guarantee that quietly stops
/// holding.
#[test]
fn renaming_the_result_type_rewrites_the_query_signature_too() {
    let document = document();
    let ranges = rename_ranges(&document, position_at("LoyaltyFeeSummary", 1), "FeeRollup")
        .expect("the result type should be renameable");

    assert_eq!(
        ranges.len(),
        2,
        "expected the `type` declaration and the query's return position, got {ranges:?}",
    );

    // Both ranges must actually cover the old name — a rename computed
    // against the wrong offsets rewrites the wrong text, which is the
    // failure mode `rename.rs` calls hardest to notice in a diff.
    for range in &ranges {
        let start = position_to_offset(SCHEMA, range.start).expect("range start should resolve");
        let end = position_to_offset(SCHEMA, range.end).expect("range end should resolve");
        assert_eq!(&SCHEMA[start..end], "LoyaltyFeeSummary");
    }
}

/// The mirror case: renaming a query's own name touches only its
/// declaration, because nothing in the language references a query by name
/// from anywhere else.
#[test]
fn renaming_the_query_itself_touches_only_its_declaration() {
    let document = document();
    let ranges = rename_ranges(&document, position_at("loyaltyFeeSummary", 1), "feeRollup")
        .expect("a query name should be renameable");

    assert_eq!(ranges.len(), 1, "got {ranges:?}");
}
