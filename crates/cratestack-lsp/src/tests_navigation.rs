//! Navigation coverage for `.cstack`: go-to-definition on every reference
//! site, and find-all-references for declarations and relation fields.

use std::str::FromStr;

use cratestack_core::{Schema, SourceSpan};
use tower_lsp_server::ls_types::Uri;

use crate::analyze::analyze_document;
use crate::definition::{definition_location, mixin_use_target_span};
use crate::references::reference_spans_at;
use crate::symbol_target::{SymbolTarget, symbol_target_at};
use crate::text::position_to_offset;

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

fn parse() -> Schema {
    let uri = Uri::from_str("file:///schema.cstack").expect("uri should parse");
    let (schema, diagnostics) = analyze_document(&uri, SCHEMA);
    assert!(
        diagnostics.is_empty(),
        "fixture should parse: {diagnostics:?}"
    );
    schema.expect("schema should parse")
}

/// Start offset of the `occurrence`-th (1-based) appearance of `needle`.
/// Spans are inclusive of their start, so this doubles as a cursor position
/// sitting on the token.
fn offset_of(needle: &str, occurrence: usize) -> usize {
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

fn text_at(span: SourceSpan) -> &'static str {
    &SCHEMA[span.start..span.end]
}

fn spans_at(needle: &str, occurrence: usize, include_declaration: bool) -> Vec<SourceSpan> {
    let schema = parse();
    reference_spans_at(
        SCHEMA,
        &schema,
        offset_of(needle, occurrence),
        include_declaration,
    )
    .expect("symbol should resolve")
}

/// An `enum` is a referenceable declaration like a model or a type; before
/// enums were added to `declaration_span`, Ctrl+Click on a `Role` field type
/// resolved to nothing at all.
#[test]
fn resolves_enum_type_reference_to_the_enum_declaration() {
    let schema = parse();
    let uri = Uri::from_str("file:///schema.cstack").expect("uri should parse");
    let location = definition_location(&uri, SCHEMA, &schema, offset_of("Role", 2))
        .expect("enum type reference should resolve");

    let start = position_to_offset(SCHEMA, location.range.start).expect("start should resolve");
    assert_eq!(start, offset_of("Role", 1));
}

/// `@use(Timestamps)` is erased from `Model::attributes` by
/// `expand_model_mixins`, so this only works if the reference site is recovered
/// from source text.
#[test]
fn resolves_mixin_use_directive_to_the_mixin_declaration() {
    let schema = parse();
    let span = mixin_use_target_span(SCHEMA, &schema, offset_of("Timestamps", 2))
        .expect("@use directive should resolve");

    assert_eq!(text_at(span), "Timestamps");
    assert_eq!(span.start, offset_of("Timestamps", 1));
}

/// The relation is navigable from the related model's end: asking for
/// references of `User.id` surfaces the `references:[id]` site on `Post`.
#[test]
fn references_of_a_related_field_include_the_relation_attribute_site() {
    let spans = spans_at("id Int @id", 1, true);
    let rendered = spans.iter().map(|span| span.line).collect::<Vec<_>>();

    assert_eq!(spans.len(), 2, "declaration plus the relation site");
    assert!(spans.iter().all(|span| text_at(*span) == "id"));
    assert_eq!(rendered, vec![15, 23]);
}

/// `id` is declared on both `User` and `Post`. Field references are qualified
/// by their owner, so asking about one must not collect the other — a plain
/// name match would return both declarations.
#[test]
fn field_references_do_not_leak_across_models_sharing_a_field_name() {
    let spans = spans_at("id Int @id", 1, true);
    let post_id_offset = offset_of("id Int @id", 2);

    assert!(
        !spans.iter().any(|span| span.start == post_id_offset),
        "User.id references must not include Post.id",
    );
}

#[test]
fn references_of_a_local_relation_field_include_the_fields_list_entry() {
    let spans = spans_at("authorId", 1, true);

    assert_eq!(spans.len(), 2);
    assert!(spans.iter().all(|span| text_at(*span) == "authorId"));
}

#[test]
fn mixin_references_include_the_use_directive() {
    let spans = spans_at("Timestamps", 1, true);

    assert_eq!(spans.len(), 2);
    assert_eq!(spans[0].start, offset_of("Timestamps", 1));
    assert_eq!(spans[1].start, offset_of("Timestamps", 2));
}

#[test]
fn excluding_the_declaration_drops_exactly_the_declaring_span() {
    let with_declaration = spans_at("User", 1, true);
    let without = spans_at("User", 1, false);

    assert_eq!(with_declaration.len(), without.len() + 1);
    assert!(
        !without
            .iter()
            .any(|span| span.start == offset_of("User", 1))
    );
}

/// A cursor inside a `mixin` block resolves to the mixin, not to whichever
/// model inlined its fields — `expand_model_mixins` clones mixin fields into
/// every consuming model while keeping the mixin's spans, so both match.
#[test]
fn mixin_fields_are_owned_by_the_mixin_not_the_inlining_model() {
    let schema = parse();
    let target = symbol_target_at(SCHEMA, &schema, offset_of("createdAt", 1))
        .expect("mixin field should resolve");

    assert_eq!(
        target,
        SymbolTarget::Field {
            owner: "Timestamps".to_owned(),
            field: "createdAt".to_owned(),
        }
    );
}
