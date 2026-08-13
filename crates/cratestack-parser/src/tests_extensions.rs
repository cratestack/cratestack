#![cfg(test)]

//! Tests for `extension <name> { }` (cratestack#153) — the schema-level
//! declaration that a `.cstack` file opts into a framework/database
//! capability. This ticket is parse-and-record only: no codegen consumes
//! `declared_extensions` yet (that's cratestack#161 onward), so these tests
//! only assert on the parsed `Schema` shape, not any generated behavior.

use super::parse_schema;
use cratestack_core::ExtensionKind;

#[test]
fn extension_rate_limit_is_recorded_on_the_schema() {
    let schema = parse_schema(
        r#"
extension rate_limit {
}

model Widget {
  id Int @id
}
"#,
    )
    .expect("`extension rate_limit { }` should parse");

    assert_eq!(
        schema.declared_extensions,
        [ExtensionKind::RateLimit].into_iter().collect(),
    );
}

#[test]
fn extension_pgvector_is_recorded_on_the_schema() {
    let schema = parse_schema(
        r#"
extension pgvector {
}

model Widget {
  id Int @id
}
"#,
    )
    .expect("`extension pgvector { }` should parse");

    assert_eq!(
        schema.declared_extensions,
        [ExtensionKind::Pgvector].into_iter().collect(),
    );
}

#[test]
fn both_default_extensions_can_be_declared_together() {
    let schema = parse_schema(
        r#"
extension rate_limit {
}

extension pgvector {
}

model Widget {
  id Int @id
}
"#,
    )
    .expect("both default extensions should be declarable in the same schema");

    assert_eq!(
        schema.declared_extensions,
        [ExtensionKind::RateLimit, ExtensionKind::Pgvector]
            .into_iter()
            .collect(),
    );
}

#[test]
fn schema_with_no_extension_blocks_has_an_empty_declared_extensions_set() {
    let schema = parse_schema(
        r#"
model Widget {
  id Int @id
}
"#,
    )
    .expect("a schema without any `extension` block should still parse");

    assert!(
        schema.declared_extensions.is_empty(),
        "a schema declaring no extensions must not gain any implicitly: {:?}",
        schema.declared_extensions,
    );
}

#[test]
fn declaring_the_same_extension_twice_is_idempotent() {
    let schema = parse_schema(
        r#"
extension rate_limit {
}

extension rate_limit {
}

model Widget {
  id Int @id
}
"#,
    )
    .expect("declaring the same extension twice should not be a parse error");

    assert_eq!(
        schema.declared_extensions,
        [ExtensionKind::RateLimit].into_iter().collect(),
    );
}

#[test]
fn unknown_extension_name_is_a_parse_error() {
    let err = parse_schema(
        r#"
extension made_up {
}

model Widget {
  id Int @id
}
"#,
    )
    .expect_err("an unrecognized extension name must be rejected, not silently accepted");

    let message = err.to_string();
    assert!(
        message.contains("unknown extension"),
        "error should call out the unknown extension, got: {message}",
    );
    assert!(
        message.contains("made_up"),
        "error should name the offending identifier, got: {message}",
    );
    assert!(
        message.contains("rate_limit") && message.contains("pgvector"),
        "error should list the valid extension names, got: {message}",
    );
}

#[test]
fn extension_block_header_must_end_with_a_brace() {
    let err = parse_schema(
        r#"
extension rate_limit

model Widget {
  id Int @id
}
"#,
    )
    .expect_err("an `extension` header without a `{` body should be rejected");

    assert!(
        err.to_string().contains("extension"),
        "error should mention the malformed extension block, got: {err}",
    );
}
