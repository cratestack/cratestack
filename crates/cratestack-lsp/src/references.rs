use cratestack_core::{Schema, SourceSpan};

use crate::mixin_use::mixin_use_names;
use crate::relation_parse::relation_attribute_spans;
use crate::symbol_target::{SymbolTarget, symbol_target_at};
use crate::type_ref::collect_type_ref_spans;

/// Resolve the symbol under `offset` and return every span mentioning it.
///
/// Shared by `textDocument/references` and `textDocument/documentHighlight` so
/// the two can never disagree about what counts as a mention.
pub(crate) fn reference_spans_at(
    text: &str,
    schema: &Schema,
    offset: usize,
    include_declaration: bool,
) -> Option<Vec<SourceSpan>> {
    let target = symbol_target_at(text, schema, offset)?;
    let mut spans = reference_spans(text, schema, &target);
    if !include_declaration && let Some(declaration) = declaration_span_of(schema, &target) {
        spans.retain(|span| span.start != declaration.start || span.end != declaration.end);
    }
    Some(spans)
}

/// Every span in the document that mentions `target`, declaration first.
///
/// Spans are de-duplicated and sorted by position so the caller can render them
/// in file order. The declaration's own span is included; callers that honour
/// `context.includeDeclaration = false` filter it out via `declaration_span`.
pub(crate) fn reference_spans(
    text: &str,
    schema: &Schema,
    target: &SymbolTarget,
) -> Vec<SourceSpan> {
    let mut spans = match target {
        SymbolTarget::Declaration(name) => declaration_references(text, schema, name),
        SymbolTarget::Field { owner, field } => field_references(schema, owner, field),
    };
    spans.sort_by_key(|span| (span.start, span.end));
    spans.dedup_by_key(|span| (span.start, span.end));
    spans
}

/// The declaring occurrence of `target`, so `includeDeclaration = false` can
/// drop exactly one span rather than guessing which of the matches it is.
pub(crate) fn declaration_span_of(schema: &Schema, target: &SymbolTarget) -> Option<SourceSpan> {
    match target {
        SymbolTarget::Declaration(name) => declaration_name_span(schema, name),
        SymbolTarget::Field { owner, field } => field_name_span(schema, owner, field),
    }
}

fn declaration_references(text: &str, schema: &Schema, name: &str) -> Vec<SourceSpan> {
    let mut spans = Vec::new();
    if let Some(span) = declaration_name_span(schema, name) {
        spans.push(span);
    }

    let field_types = schema
        .models
        .iter()
        .flat_map(|model| &model.fields)
        .chain(schema.types.iter().flat_map(|ty| &ty.fields))
        .chain(schema.mixins.iter().flat_map(|mixin| &mixin.fields));
    for field in field_types {
        collect_type_ref_spans(&field.ty, name, &mut spans);
    }
    for procedure in &schema.procedures {
        collect_type_ref_spans(&procedure.return_type, name, &mut spans);
        for arg in &procedure.args {
            collect_type_ref_spans(&arg.ty, name, &mut spans);
        }
    }
    // A query's signature references types exactly as a procedure's does
    // (cratestack#867) — omitting it would mean renaming a `type` silently
    // skipped the query that returns it, leaving the schema uncompilable.
    for query in &schema.queries {
        collect_type_ref_spans(&query.result_type, name, &mut spans);
        for arg in &query.args {
            collect_type_ref_spans(&arg.ty, name, &mut spans);
        }
    }

    // `@use(Timestamps)` is a reference to the mixin just as much as a field
    // type is a reference to a model.
    spans.extend(
        mixin_use_names(text)
            .into_iter()
            .filter(|used| used.name == name)
            .map(|used| used.span),
    );

    spans
}

fn field_references(schema: &Schema, owner: &str, field: &str) -> Vec<SourceSpan> {
    let mut spans = Vec::new();
    if let Some(span) = field_name_span(schema, owner, field) {
        spans.push(span);
    }

    for model in &schema.models {
        for model_field in &model.fields {
            let Some(relation) = relation_attribute_spans(&model_field.attributes) else {
                continue;
            };
            // `fields:` names columns on the model that holds the attribute.
            if model.name == owner {
                spans.extend(
                    relation
                        .fields
                        .iter()
                        .filter(|entry| entry.name == field)
                        .map(|entry| entry.span),
                );
            }
            // `references:` names columns on the model the field's type points
            // at, which is what makes a relation navigable from both ends.
            if model_field.ty.name == owner {
                spans.extend(
                    relation
                        .references
                        .iter()
                        .filter(|entry| entry.name == field)
                        .map(|entry| entry.span),
                );
            }
        }
    }

    spans
}

fn declaration_name_span(schema: &Schema, name: &str) -> Option<SourceSpan> {
    schema
        .models
        .iter()
        .find(|model| model.name == name)
        .map(|model| model.name_span)
        .or_else(|| {
            schema
                .types
                .iter()
                .find(|ty| ty.name == name)
                .map(|ty| ty.name_span)
        })
        .or_else(|| {
            schema
                .mixins
                .iter()
                .find(|mixin| mixin.name == name)
                .map(|mixin| mixin.name_span)
        })
        .or_else(|| {
            schema
                .enums
                .iter()
                .find(|decl| decl.name == name)
                .map(|decl| decl.name_span)
        })
        .or_else(|| {
            schema
                .procedures
                .iter()
                .find(|procedure| procedure.name == name)
                .map(|procedure| procedure.name_span)
        })
        .or_else(|| {
            schema
                .queries
                .iter()
                .find(|query| query.name == name)
                .map(|query| query.name_span)
        })
}

fn field_name_span(schema: &Schema, owner: &str, field: &str) -> Option<SourceSpan> {
    let owned_fields = schema
        .models
        .iter()
        .find(|model| model.name == owner)
        .map(|model| &model.fields)
        .or_else(|| {
            schema
                .types
                .iter()
                .find(|ty| ty.name == owner)
                .map(|ty| &ty.fields)
        })
        .or_else(|| {
            schema
                .mixins
                .iter()
                .find(|mixin| mixin.name == owner)
                .map(|mixin| &mixin.fields)
        });

    if let Some(fields) = owned_fields {
        return fields
            .iter()
            .find(|candidate| candidate.name == field)
            .map(|candidate| candidate.name_span);
    }

    schema
        .enums
        .iter()
        .find(|decl| decl.name == owner)
        .and_then(|decl| decl.variants.iter().find(|variant| variant.name == field))
        .map(|variant| variant.span)
}
