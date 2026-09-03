use cratestack_core::{Schema, SourceSpan};
use tower_lsp_server::ls_types::{Location, Uri};

use crate::mixin_use::mixin_use_names;
use crate::relation_parse::relation_attribute_spans;
use crate::text::{range_from_offsets, span_contains, word_at_offset};
use crate::type_ref::nested_type_reference_name_at_offset;

pub(crate) fn definition_location(
    uri: &Uri,
    text: &str,
    schema: &Schema,
    offset: usize,
) -> Option<Location> {
    let span = relation_target_span(schema, offset)
        .or_else(|| mixin_use_target_span(text, schema, offset))
        .or_else(|| type_reference_target_span(schema, offset))
        .or_else(|| word_at_offset(text, offset).and_then(|word| declaration_span(schema, word)))?;
    Some(Location {
        uri: uri.clone(),
        range: range_from_offsets(text, span.start, span.end),
    })
}

pub(crate) fn declaration_span(schema: &Schema, word: &str) -> Option<SourceSpan> {
    if let Some(datasource) = &schema.datasource
        && datasource.name == word
    {
        return Some(datasource.span);
    }
    if let Some(auth) = &schema.auth {
        if auth.name == word {
            return Some(auth.span);
        }
        if let Some(field) = auth.fields.iter().find(|field| field.name == word) {
            return Some(field.name_span);
        }
    }
    for model in &schema.models {
        if model.name == word {
            return Some(model.name_span);
        }
        if let Some(field) = model.fields.iter().find(|field| field.name == word) {
            return Some(field.name_span);
        }
    }
    for ty in &schema.types {
        if ty.name == word {
            return Some(ty.name_span);
        }
        if let Some(field) = ty.fields.iter().find(|field| field.name == word) {
            return Some(field.name_span);
        }
    }
    // Mixins and enums are declarable, referenceable names like models and
    // types are — `@use(Timestamps)` and a `Role` field type both need to
    // resolve — but were absent here, so Ctrl+Click on either did nothing.
    for mixin in &schema.mixins {
        if mixin.name == word {
            return Some(mixin.name_span);
        }
        if let Some(field) = mixin.fields.iter().find(|field| field.name == word) {
            return Some(field.name_span);
        }
    }
    for decl in &schema.enums {
        if decl.name == word {
            return Some(decl.name_span);
        }
        if let Some(variant) = decl.variants.iter().find(|variant| variant.name == word) {
            return Some(variant.span);
        }
    }
    for procedure in &schema.procedures {
        if procedure.name == word {
            return Some(procedure.name_span);
        }
        if let Some(arg) = procedure.args.iter().find(|arg| arg.name == word) {
            return Some(arg.name_span);
        }
    }
    crate::query_symbols::declaration_span(schema, word)
}

/// `@use(Timestamps)` on a model resolves to the `mixin Timestamps`
/// declaration. Read from text, not the IR — see `mixin_use::mixin_use_names`
/// for why the attribute is gone by the time the schema arrives.
pub(crate) fn mixin_use_target_span(
    text: &str,
    schema: &Schema,
    offset: usize,
) -> Option<SourceSpan> {
    mixin_use_names(text)
        .into_iter()
        .find(|used| span_contains(used.span, offset))
        .and_then(|used| declaration_span(schema, &used.name))
}

pub(crate) fn type_reference_target_span(schema: &Schema, offset: usize) -> Option<SourceSpan> {
    for model in &schema.models {
        for field in &model.fields {
            if span_contains(field.ty.name_span, offset) {
                return declaration_span(schema, &field.ty.name);
            }
        }
    }
    for ty in &schema.types {
        for field in &ty.fields {
            if span_contains(field.ty.name_span, offset) {
                return declaration_span(schema, &field.ty.name);
            }
        }
    }
    // Mixin bodies hold ordinary fields whose types deserve the same
    // navigation as a model's or a type's.
    for mixin in &schema.mixins {
        for field in &mixin.fields {
            if span_contains(field.ty.name_span, offset) {
                return declaration_span(schema, &field.ty.name);
            }
        }
    }
    for procedure in &schema.procedures {
        if let Some(target) = nested_type_reference_name_at_offset(&procedure.return_type, offset) {
            return declaration_span(schema, target);
        }
        for arg in &procedure.args {
            if let Some(target) = nested_type_reference_name_at_offset(&arg.ty, offset) {
                return declaration_span(schema, target);
            }
        }
    }
    // A query's result type is a reference to a `type` declaration, so
    // go-to-definition on it must land on that block (cratestack#867).
    if let Some(target) = crate::query_symbols::type_reference_at(schema, offset) {
        return declaration_span(schema, target);
    }
    None
}

pub(crate) fn relation_target_span(schema: &Schema, offset: usize) -> Option<SourceSpan> {
    for model in &schema.models {
        for field in &model.fields {
            let Some(relation) = relation_attribute_spans(&field.attributes) else {
                continue;
            };
            if let Some(name) = relation
                .fields
                .iter()
                .find(|name| span_contains(name.span, offset))
                && let Some(target) = model
                    .fields
                    .iter()
                    .find(|candidate| candidate.name == name.name)
            {
                return Some(target.name_span);
            }
            if let Some(name) = relation
                .references
                .iter()
                .find(|name| span_contains(name.span, offset))
            {
                let Some(related_model) = schema
                    .models
                    .iter()
                    .find(|candidate| candidate.name == field.ty.name)
                else {
                    continue;
                };
                if let Some(target) = related_model
                    .fields
                    .iter()
                    .find(|candidate| candidate.name == name.name)
                {
                    return Some(target.name_span);
                }
            }
        }
    }
    None
}
