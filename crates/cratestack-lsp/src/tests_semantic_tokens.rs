//! Semantic-token coverage: the point of these tokens is telling identifiers
//! apart, which a TextMate grammar cannot do, so the tests assert on what each
//! identifier *resolves to* rather than on raw counts.

use std::str::FromStr;

use tower_lsp_server::ls_types::{SemanticToken, SemanticTokenType, Uri};

use crate::analyze::analyze_document;
use crate::semantic_tokens::{LEGEND, semantic_tokens};

const SCHEMA: &str = r#"datasource db {
  provider = "postgresql"
}

mixin Timestamps {
  createdAt DateTime
}

enum Role {
  Admin
  Member
}

model User {
  id Int @id
  role Role
  @use(Timestamps)
}

model Post {
  id Int @id
  authorId Int
  author User @relation(fields:[authorId],references:[id])
}
"#;

/// Decodes the delta-encoded stream back into `(text, token type)` pairs the
/// way a client does, so the tests exercise the encoding rather than trusting
/// it.
fn decode() -> Vec<(String, SemanticTokenType)> {
    let uri = Uri::from_str("file:///schema.cstack").expect("uri should parse");
    let (schema, diagnostics) = analyze_document(&uri, SCHEMA);
    assert!(
        diagnostics.is_empty(),
        "fixture should parse: {diagnostics:?}"
    );
    let schema = schema.expect("schema should parse");
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
        let source = lines[line];
        let text = source
            .chars()
            .skip(character)
            .take(length as usize)
            .collect::<String>();
        decoded.push((text, LEGEND[token_type as usize].clone()));
    }
    decoded
}

fn kinds_of(needle: &str) -> Vec<SemanticTokenType> {
    decode()
        .into_iter()
        .filter(|(text, _)| text == needle)
        .map(|(_, kind)| kind)
        .collect()
}

/// The headline case: four bare capitalised words that only a resolved schema
/// can tell apart. A grammar sees one category here; this sees four.
#[test]
fn identifiers_resolve_to_distinct_token_types() {
    assert_eq!(kinds_of("User"), vec![SemanticTokenType::STRUCT; 2]);
    assert_eq!(kinds_of("Role"), vec![SemanticTokenType::ENUM; 2]);
    assert_eq!(
        kinds_of("Timestamps"),
        vec![SemanticTokenType::INTERFACE; 2],
        "mixin declaration and its @use reference",
    );
    assert_eq!(
        kinds_of("Int"),
        vec![SemanticTokenType::TYPE; 3],
        "builtin scalars stay `type`, unlike declared names",
    );
}

#[test]
fn enum_variants_and_fields_are_distinguished_from_their_declarations() {
    assert_eq!(kinds_of("Admin"), vec![SemanticTokenType::ENUM_MEMBER]);
    assert_eq!(kinds_of("role"), vec![SemanticTokenType::PROPERTY]);
    assert_eq!(kinds_of("author"), vec![SemanticTokenType::PROPERTY]);
}

/// Only the `@name` head is a decorator; the arguments carry their own tokens,
/// so `authorId` inside `@relation(...)` stays a property.
#[test]
fn attribute_heads_are_decorators_and_relation_columns_stay_properties() {
    assert_eq!(
        kinds_of("@relation"),
        vec![SemanticTokenType::DECORATOR],
        "the head only, not the whole attribute",
    );
    assert_eq!(kinds_of("@id"), vec![SemanticTokenType::DECORATOR; 2]);
    assert_eq!(
        kinds_of("authorId"),
        vec![SemanticTokenType::PROPERTY; 2],
        "declaration plus the relation `fields:` entry",
    );
}

/// `expand_model_mixins` clones each mixin field into every consuming model
/// while keeping the mixin's spans, so the same span is collected twice.
/// Emitting it twice would produce a zero-width delta and a duplicated token.
#[test]
fn inlined_mixin_fields_are_not_emitted_twice() {
    assert_eq!(
        kinds_of("createdAt"),
        vec![SemanticTokenType::PROPERTY],
        "declared once in the mixin, inlined into User, emitted once",
    );
}

/// The protocol requires non-decreasing positions; a client decoding an
/// out-of-order stream silently mis-colours everything after the fault.
#[test]
fn the_encoded_stream_is_monotonic() {
    let uri = Uri::from_str("file:///schema.cstack").expect("uri should parse");
    let (schema, _) = analyze_document(&uri, SCHEMA);
    let schema = schema.expect("schema should parse");

    let mut line = 0i64;
    let mut character = 0i64;
    for token in semantic_tokens(SCHEMA, &schema) {
        line += i64::from(token.delta_line);
        character = if token.delta_line == 0 {
            character + i64::from(token.delta_start)
        } else {
            i64::from(token.delta_start)
        };
        assert!(token.length > 0, "zero-length tokens are not renderable");
        assert!(character >= 0);
    }
    assert!(line > 0);
}
