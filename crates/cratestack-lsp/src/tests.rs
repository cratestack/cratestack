use std::str::FromStr;

use tower_lsp_server::ls_types::{Position, SymbolKind, Uri};

use crate::analyze::analyze_document;
use crate::definition::{declaration_span, relation_target_span, type_reference_target_span};
use crate::document_symbols::document_symbols;
use crate::hover::locate_symbol;
use crate::text::{offset_to_position, position_to_offset, word_at_offset};

#[test]
fn converts_utf16_positions_round_trip() {
    let text = "/// User docs\nmodel User {\n  emoji String\n}\n";
    let offset = position_to_offset(
        text,
        Position {
            line: 2,
            character: 2,
        },
    )
    .expect("position should resolve");

    assert_eq!(
        offset_to_position(text, offset),
        Position {
            line: 2,
            character: 2
        }
    );
}

#[test]
fn returns_hoverable_symbol_docs_from_schema() {
    let text = "/// User docs\nmodel User {\n  /// Email docs\n  email String @id\n}\n";
    let uri = Uri::from_str("file:///schema.cstack").expect("uri should parse");
    let (schema, diagnostics) = analyze_document(&uri, text);

    assert!(diagnostics.is_empty());
    let schema = schema.expect("schema should parse");
    let offset = text.find("email").expect("field should exist");
    let symbol = locate_symbol(&schema, offset).expect("symbol should resolve");

    assert_eq!(symbol.kind, "field");
    assert_eq!(symbol.docs, vec!["Email docs".to_owned()]);
}

#[test]
fn extracts_identifier_at_offset_for_definition_lookup() {
    let text = "model User {\n  userId Int @id\n}\n";
    let offset = text.find("userId").expect("identifier should exist") + 2;

    assert_eq!(word_at_offset(text, offset), Some("userId"));
}

#[test]
fn resolves_declaration_span_by_name() {
    let text = "type FeedInput {\n  limit Int\n}\nprocedure getFeed(args: FeedInput): FeedInput\n";
    let uri = Uri::from_str("file:///schema.cstack").expect("uri should parse");
    let (schema, diagnostics) = analyze_document(&uri, text);

    assert!(diagnostics.is_empty());
    let schema = schema.expect("schema should parse");
    let span = declaration_span(&schema, "FeedInput").expect("type should resolve");

    assert_eq!(text[span.start..span.end].lines().next(), Some("FeedInput"));
}

#[test]
fn builds_hierarchical_document_symbols() {
    let text = "model User {\n  id Int @id\n}\n";
    let uri = Uri::from_str("file:///schema.cstack").expect("uri should parse");
    let (schema, diagnostics) = analyze_document(&uri, text);

    assert!(diagnostics.is_empty());
    let schema = schema.expect("schema should parse");
    let symbols = document_symbols(text, &schema);

    assert_eq!(symbols.len(), 1);
    assert_eq!(symbols[0].kind, SymbolKind::STRUCT);
    assert_eq!(symbols[0].children.as_ref().expect("children").len(), 1);
}

#[test]
fn resolves_relation_fields_and_references_to_the_correct_field_names() {
    let text = "model User {\n  id Int @id\n}\n\nmodel Post {\n  id Int @id\n  authorId Int\n  author User @relation(fields:[authorId],references:[id])\n}\n";
    let uri = Uri::from_str("file:///schema.cstack").expect("uri should parse");
    let (schema, diagnostics) = analyze_document(&uri, text);

    assert!(diagnostics.is_empty());
    let schema = schema.expect("schema should parse");

    let local_offset = text
        .rfind("authorId")
        .expect("relation authorId should exist");
    let reference_offset = text.rfind("id]").expect("reference id should exist");

    let local_span =
        relation_target_span(&schema, local_offset).expect("local field should resolve");
    let reference_span =
        relation_target_span(&schema, reference_offset).expect("reference field should resolve");

    assert_eq!(&text[local_span.start..local_span.end], "authorId");
    assert_eq!(&text[reference_span.start..reference_span.end], "id");
    assert!(reference_span.start < local_span.start);
}

#[test]
fn resolves_type_reference_to_declaration_name_span() {
    let text = "type FeedInput {\n  limit Int\n}\nprocedure getFeed(args: FeedInput): FeedInput\n";
    let uri = Uri::from_str("file:///schema.cstack").expect("uri should parse");
    let (schema, diagnostics) = analyze_document(&uri, text);

    assert!(diagnostics.is_empty());
    let schema = schema.expect("schema should parse");
    let offset = text.rfind("FeedInput").expect("return type should exist");
    let span =
        type_reference_target_span(&schema, offset).expect("type reference should resolve");

    assert_eq!(&text[span.start..span.end], "FeedInput");
    assert_eq!(
        span.start,
        text.find("FeedInput")
            .expect("type declaration should exist")
    );
}

#[test]
fn narrows_unknown_relation_field_diagnostic_to_the_relation_name() {
    let text = "model User {\n  id Int @id\n}\n\nmodel Post {\n  id Int @id\n  authorId Int\n  author User @relation(fields:[ownerId],references:[id])\n}\n";
    let uri = Uri::from_str("file:///schema.cstack").expect("uri should parse");
    let (_schema, diagnostics) = analyze_document(&uri, text);

    assert_eq!(diagnostics.len(), 1);
    let diagnostic = &diagnostics[0];
    let start = position_to_offset(text, diagnostic.range.start).expect("start should resolve");
    let end = position_to_offset(text, diagnostic.range.end).expect("end should resolve");

    assert_eq!(&text[start..end], "ownerId");
}

#[test]
fn includes_procedure_args_as_document_symbol_children() {
    let text =
        "/// Feed docs\n/// @param limit Maximum items\nprocedure getFeed(limit: Int): Int\n";
    let uri = Uri::from_str("file:///schema.cstack").expect("uri should parse");
    let (schema, diagnostics) = analyze_document(&uri, text);

    assert!(diagnostics.is_empty());
    let schema = schema.expect("schema should parse");
    let symbols = document_symbols(text, &schema);
    let args = symbols[0].children.as_ref().expect("children should exist");

    assert_eq!(args.len(), 1);
    assert_eq!(args[0].name, "limit");
    assert_eq!(args[0].kind, SymbolKind::VARIABLE);
}
