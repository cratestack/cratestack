//! Editor coverage for the `query` block (cratestack#867): tokens,
//! outline, hover, completion and navigation.
//!
//! Rename lives in the sibling
//! [`tests_queries_rename`](crate::tests_queries_rename) — split for the
//! workspace's 200-line ceiling, and a reasonable seam anyway: rename is
//! the one request here that *writes*, so it is held to a stricter
//! standard than the read-only surfaces below.
//!
//! Every surface below already handled `procedure`; a `query` reaching
//! none of them would make the construct second-class in the editor —
//! visible in the file, invisible to the language server. The one place it
//! is deliberately *not* handled is the SQL body: it is opaque text here,
//! and colouring it would mean embedding a SQL highlighter for a dialect
//! the framework never parses.

use std::str::FromStr;

use cratestack_core::Schema;
use tower_lsp_server::ls_types::{SemanticToken, SemanticTokenType, SymbolKind, Uri};

use crate::analyze::analyze_document;
use crate::completion::completion_items;
use crate::definition::declaration_span;
use crate::document_symbols::document_symbols;
use crate::hover::locate_symbol;
use crate::rename::prepare_rename;
use crate::semantic_tokens::{LEGEND, semantic_tokens};
use crate::state::next_document_state;

pub(crate) const SCHEMA: &str = r#"datasource db {
  provider = "postgresql"
}

type LoyaltyFeeSummary {
  total Int
  thisMonth Int
}

/// Monthly loyalty fee rollup.
query loyaltyFeeSummary(userId: String, cutoff: DateTime): LoyaltyFeeSummary
  @@sql("SELECT 0::bigint AS \"total\", 0::bigint AS \"thisMonth\" WHERE a = $1 AND b >= $2")
  @allow(auth() != null)
"#;

pub(crate) fn parse() -> Schema {
    let uri = Uri::from_str("file:///schema.cstack").expect("uri should parse");
    let (schema, diagnostics) = analyze_document(&uri, SCHEMA);
    assert!(
        diagnostics.is_empty(),
        "fixture should parse: {diagnostics:?}"
    );
    schema.expect("schema should parse")
}

/// Start offset of the `occurrence`-th (1-based) appearance of `needle`.
pub(crate) fn offset_of(needle: &str, occurrence: usize) -> usize {
    let mut search_from = 0usize;
    let mut found = 0usize;
    for _ in 0..occurrence {
        found = SCHEMA[search_from..]
            .find(needle)
            .map(|index| search_from + index)
            .expect("needle should exist");
        search_from = found + 1;
    }
    found
}

fn decoded_tokens() -> Vec<(String, SemanticTokenType)> {
    let schema = parse();
    let tokens = semantic_tokens(SCHEMA, &schema);
    let lines = SCHEMA.split('\n').collect::<Vec<_>>();
    let mut decoded = Vec::new();
    let mut line = 0usize;
    let mut character = 0usize;

    for SemanticToken {
        delta_line,
        delta_start,
        length,
        token_type,
        ..
    } in tokens
    {
        line += delta_line as usize;
        character = if delta_line == 0 {
            character + delta_start as usize
        } else {
            delta_start as usize
        };
        let text = lines[line]
            .chars()
            .skip(character)
            .take(length as usize)
            .collect::<String>();
        decoded.push((text, LEGEND[token_type as usize].clone()));
    }
    decoded
}

#[test]
fn the_query_name_and_parameters_get_semantic_tokens() {
    let decoded = decoded_tokens();
    assert!(
        decoded.contains(&("loyaltyFeeSummary".to_owned(), SemanticTokenType::FUNCTION)),
        "query name should colour as a function: {decoded:?}",
    );
    assert!(
        decoded.contains(&("userId".to_owned(), SemanticTokenType::PARAMETER)),
        "query parameters should colour as parameters: {decoded:?}",
    );
    assert!(
        decoded.contains(&("LoyaltyFeeSummary".to_owned(), SemanticTokenType::CLASS))
            || decoded.iter().any(|(text, _)| text == "LoyaltyFeeSummary"),
        "the result type reference should be tokenised: {decoded:?}",
    );
}

#[test]
fn the_query_appears_in_the_document_outline_with_its_parameters() {
    let schema = parse();
    let symbols = document_symbols(SCHEMA, &schema);
    let query = symbols
        .iter()
        .find(|symbol| symbol.name == "loyaltyFeeSummary")
        .expect("query should appear in the outline");

    assert_eq!(query.kind, SymbolKind::FUNCTION);
    assert_eq!(query.detail.as_deref(), Some("query -> LoyaltyFeeSummary"));
    let children = query
        .children
        .as_ref()
        .expect("query should list its parameters");
    assert_eq!(
        children.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        vec!["userId", "cutoff"],
    );
}

#[test]
fn hovering_the_query_name_reports_it_as_a_query() {
    let schema = parse();
    let symbol = locate_symbol(&schema, offset_of("loyaltyFeeSummary", 1))
        .expect("hovering a query name should resolve");

    assert_eq!(symbol.kind, "query");
    assert_eq!(symbol.name, "loyaltyFeeSummary");
    assert_eq!(symbol.detail, "query -> LoyaltyFeeSummary");
    assert_eq!(symbol.docs, vec!["Monthly loyalty fee rollup."]);
}

#[test]
fn a_query_parameter_resolves_to_its_own_declaration() {
    // This is what makes an `@allow(... == userId)` predicate navigable.
    let schema = parse();
    let span = declaration_span(&schema, "userId").expect("parameter should resolve");
    assert_eq!(&SCHEMA[span.start..span.end], "userId");
}

#[test]
fn the_query_name_is_offered_in_completions() {
    let schema = parse();
    let items = completion_items(Some(&schema));
    let query = items
        .iter()
        .find(|item| item.label == "loyaltyFeeSummary")
        .expect("query should be completable");
    assert_eq!(query.detail.as_deref(), Some("query"));
    assert!(
        items.iter().any(|item| item.label == "query"),
        "the `query` keyword itself should be completable",
    );
}

#[test]
fn the_query_keyword_itself_cannot_be_renamed() {
    // `query` changes how a file parses, so offering a rename box over the
    // keyword would silently change what the file means.
    let uri = Uri::from_str("file:///schema.cstack").expect("uri should parse");
    let (parsed, _) = analyze_document(&uri, SCHEMA);
    let document = next_document_state(None, SCHEMA.to_owned(), parsed);
    let keyword_line = SCHEMA[..offset_of("query loyaltyFeeSummary", 1)]
        .matches('\n')
        .count() as u32;
    let position = tower_lsp_server::ls_types::Position {
        line: keyword_line,
        character: 2,
    };

    assert!(
        prepare_rename(&document, position).is_none(),
        "the `query` keyword must not be renameable",
    );
}
