//! Semantic tokens for `.cstack`.
//!
//! These *supplement* the TextMate grammar rather than replace it. VS Code has
//! no tree-sitter API for third-party languages, so the grammar keeps doing
//! what regexes do well — keywords, strings, comments, punctuation, available
//! instantly before the server even starts — and this fills the gap regexes
//! cannot close: telling identifiers apart. `String` (builtin), `User`
//! (a model), `Role` (an enum) and `Timestamps` (a mixin) are all bare
//! capitalised words to a grammar; only a resolved schema knows which is which.

use cratestack_core::{Schema, SourceSpan, TypeRef};
use tower_lsp_server::ls_types::{SemanticToken, SemanticTokenType};

use crate::mixin_use::mixin_use_names;
use crate::relation_parse::relation_attribute_spans;
use crate::state::DocumentState;
use crate::text::offset_to_position;

/// Order is load-bearing: the client resolves `token_type` as an index into
/// this array, exactly as advertised in `SemanticTokensLegend`.
pub(crate) const LEGEND: &[SemanticTokenType] = &[
    SemanticTokenType::TYPE,
    SemanticTokenType::STRUCT,
    SemanticTokenType::ENUM,
    SemanticTokenType::INTERFACE,
    SemanticTokenType::ENUM_MEMBER,
    SemanticTokenType::PROPERTY,
    SemanticTokenType::FUNCTION,
    SemanticTokenType::PARAMETER,
    SemanticTokenType::DECORATOR,
];

const TYPE: u32 = 0;
const STRUCT: u32 = 1;
const ENUM: u32 = 2;
const INTERFACE: u32 = 3;
const ENUM_MEMBER: u32 = 4;
const PROPERTY: u32 = 5;
const FUNCTION: u32 = 6;
const PARAMETER: u32 = 7;
const DECORATOR: u32 = 8;

/// Tokens for a document, or `None` when nothing has parsed yet.
pub(crate) fn semantic_tokens_for(document: &DocumentState) -> Option<Vec<SemanticToken>> {
    let (text, schema) = document.resolved()?;
    Some(semantic_tokens(text, schema))
}

pub(crate) fn semantic_tokens(text: &str, schema: &Schema) -> Vec<SemanticToken> {
    let mut entries = Vec::new();

    for mixin in &schema.mixins {
        entries.push((mixin.name_span, INTERFACE));
        collect_fields(schema, &mixin.fields, &mut entries);
    }
    for model in &schema.models {
        entries.push((model.name_span, STRUCT));
        collect_fields(schema, &model.fields, &mut entries);
        collect_attributes(&model.attributes, &mut entries);
    }
    for ty in &schema.types {
        entries.push((ty.name_span, TYPE));
        collect_fields(schema, &ty.fields, &mut entries);
    }
    for decl in &schema.enums {
        entries.push((decl.name_span, ENUM));
        for variant in &decl.variants {
            entries.push((variant.span, ENUM_MEMBER));
        }
    }
    for procedure in &schema.procedures {
        entries.push((procedure.name_span, FUNCTION));
        collect_type_ref(schema, &procedure.return_type, &mut entries);
        for arg in &procedure.args {
            entries.push((arg.name_span, PARAMETER));
            collect_type_ref(schema, &arg.ty, &mut entries);
        }
    }
    // `query` blocks (cratestack#867) colour exactly like procedures:
    // name as FUNCTION, parameters as PARAMETER, result type resolved.
    // The SQL body is deliberately left uncoloured — it is opaque text to
    // this language server, and pretending otherwise would mean embedding
    // a SQL highlighter for a dialect the framework never parses.
    for query in &schema.queries {
        entries.push((query.name_span, FUNCTION));
        collect_type_ref(schema, &query.result_type, &mut entries);
        for arg in &query.args {
            entries.push((arg.name_span, PARAMETER));
            collect_type_ref(schema, &arg.ty, &mut entries);
        }
    }

    // `@use(...)` is erased from the IR by `expand_model_mixins`, so its span
    // comes from source text — see `mixin_use::mixin_use_names`.
    for used in mixin_use_names(text) {
        entries.push((used.span, INTERFACE));
    }

    delta_encode(text, entries)
}

fn collect_fields(
    schema: &Schema,
    fields: &[cratestack_core::Field],
    entries: &mut Vec<(SourceSpan, u32)>,
) {
    for field in fields {
        entries.push((field.name_span, PROPERTY));
        collect_type_ref(schema, &field.ty, entries);
        collect_attributes(&field.attributes, entries);
        // `@relation(fields: [...], references: [...])` entries name real
        // columns, so they colour as properties rather than as attribute text.
        if let Some(relation) = relation_attribute_spans(&field.attributes) {
            for entry in relation.fields.iter().chain(relation.references.iter()) {
                entries.push((entry.span, PROPERTY));
            }
        }
    }
}

/// Only the `@name` head of an attribute, not its arguments — the arguments
/// carry their own tokens (relation columns above) and swallowing them would
/// flatten the whole attribute into one colour.
fn collect_attributes(
    attributes: &[cratestack_core::Attribute],
    entries: &mut Vec<(SourceSpan, u32)>,
) {
    for attribute in attributes {
        let head = attribute
            .raw
            .find('(')
            .unwrap_or_else(|| attribute.raw.trim_end().len());
        if head == 0 {
            continue;
        }
        entries.push((
            SourceSpan {
                start: attribute.span.start,
                end: attribute.span.start + head,
                line: attribute.span.line,
            },
            DECORATOR,
        ));
    }
}

fn collect_type_ref(schema: &Schema, ty: &TypeRef, entries: &mut Vec<(SourceSpan, u32)>) {
    entries.push((ty.name_span, classify(schema, &ty.name)));
    for inner in &ty.generic_args {
        collect_type_ref(schema, inner, entries);
    }
}

/// What a type reference actually points at. Anything the schema does not
/// declare is a builtin scalar (`String`, `Int`, `DateTime`, `Page`, …).
fn classify(schema: &Schema, name: &str) -> u32 {
    if schema.models.iter().any(|model| model.name == name) {
        return STRUCT;
    }
    if schema.enums.iter().any(|decl| decl.name == name) {
        return ENUM;
    }
    if schema.mixins.iter().any(|mixin| mixin.name == name) {
        return INTERFACE;
    }
    TYPE
}

/// LSP wants tokens sorted and encoded relative to their predecessor, with
/// lengths and columns in UTF-16 code units.
fn delta_encode(text: &str, mut entries: Vec<(SourceSpan, u32)>) -> Vec<SemanticToken> {
    entries.sort_by_key(|(span, _)| (span.start, span.end));
    entries.dedup_by_key(|(span, _)| (span.start, span.end));

    let mut tokens = Vec::new();
    let mut previous_line = 0u32;
    let mut previous_start = 0u32;

    for (span, token_type) in entries {
        let start = offset_to_position(text, span.start);
        let end = offset_to_position(text, span.end);
        // A multi-line token is not expressible in this encoding; the protocol
        // has no length that spans a newline.
        if end.line != start.line || end.character <= start.character {
            continue;
        }
        let delta_line = start.line - previous_line;
        let delta_start = if delta_line == 0 {
            start.character - previous_start
        } else {
            start.character
        };
        tokens.push(SemanticToken {
            delta_line,
            delta_start,
            length: end.character - start.character,
            token_type,
            token_modifiers_bitset: 0,
        });
        previous_line = start.line;
        previous_start = start.character;
    }

    tokens
}
