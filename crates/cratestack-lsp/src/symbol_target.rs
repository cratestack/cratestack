use cratestack_core::Schema;

use crate::mixin_use::mixin_use_names;
use crate::relation_parse::relation_attribute_spans;
use crate::text::{span_contains, word_at_offset};
use crate::type_ref::nested_type_reference_name_at_offset;

/// What the cursor is pointing at, resolved well enough to search for every
/// other mention of the same thing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SymbolTarget {
    /// A schema-global name: model, type, enum, mixin or procedure. These are
    /// unique across a schema, so matching other mentions by name is exact.
    Declaration(String),
    /// A field, qualified by the declaration that owns it. Field names are only
    /// unique within their owner — `id` exists on nearly every model — so an
    /// unqualified name match would collect unrelated fields.
    Field { owner: String, field: String },
}

pub(crate) fn symbol_target_at(text: &str, schema: &Schema, offset: usize) -> Option<SymbolTarget> {
    relation_target(schema, offset)
        .or_else(|| mixin_use_target(text, offset))
        .or_else(|| type_reference_target(schema, offset))
        .or_else(|| declaration_target(schema, offset))
        .or_else(|| word_target(text, schema, offset))
}

/// `@relation(fields: [...], references: [...])`. The two lists point at
/// different owners: `fields` names columns on the model holding the attribute,
/// `references` names columns on the *related* model named by the field's type.
fn relation_target(schema: &Schema, offset: usize) -> Option<SymbolTarget> {
    for model in &schema.models {
        for field in &model.fields {
            let Some(relation) = relation_attribute_spans(&field.attributes) else {
                continue;
            };
            if let Some(local) = relation
                .fields
                .iter()
                .find(|entry| span_contains(entry.span, offset))
            {
                return Some(SymbolTarget::Field {
                    owner: model.name.clone(),
                    field: local.name.clone(),
                });
            }
            if let Some(remote) = relation
                .references
                .iter()
                .find(|entry| span_contains(entry.span, offset))
            {
                return Some(SymbolTarget::Field {
                    owner: field.ty.name.clone(),
                    field: remote.name.clone(),
                });
            }
        }
    }
    None
}

fn mixin_use_target(text: &str, offset: usize) -> Option<SymbolTarget> {
    mixin_use_names(text)
        .into_iter()
        .find(|used| span_contains(used.span, offset))
        .map(|used| SymbolTarget::Declaration(used.name))
}

fn type_reference_target(schema: &Schema, offset: usize) -> Option<SymbolTarget> {
    let field_types = schema
        .models
        .iter()
        .flat_map(|model| &model.fields)
        .chain(schema.types.iter().flat_map(|ty| &ty.fields))
        .chain(schema.mixins.iter().flat_map(|mixin| &mixin.fields));
    for field in field_types {
        if let Some(name) = nested_type_reference_name_at_offset(&field.ty, offset) {
            return Some(SymbolTarget::Declaration(name.to_owned()));
        }
    }
    for procedure in &schema.procedures {
        if let Some(name) = nested_type_reference_name_at_offset(&procedure.return_type, offset) {
            return Some(SymbolTarget::Declaration(name.to_owned()));
        }
        for arg in &procedure.args {
            if let Some(name) = nested_type_reference_name_at_offset(&arg.ty, offset) {
                return Some(SymbolTarget::Declaration(name.to_owned()));
            }
        }
    }
    // cratestack#867 — see `references::type_references`' matching note.
    crate::query_symbols::type_reference_at(schema, offset)
        .map(|name| SymbolTarget::Declaration(name.to_owned()))
}

/// The cursor sitting on a declaration's own name, or on one of its members.
fn declaration_target(schema: &Schema, offset: usize) -> Option<SymbolTarget> {
    // Mixins are checked before models on purpose. `expand_model_mixins` clones
    // each mixin field into every consuming model *keeping the mixin's spans*,
    // so a cursor inside a `mixin` block matches both the mixin's field and the
    // model's copy of it. The mixin is the declaration site, so it has to win —
    // otherwise the owner would be whichever model happened to be listed first.
    let owners = schema
        .mixins
        .iter()
        .map(|mixin| (&mixin.name, mixin.name_span, &mixin.fields))
        .chain(
            schema
                .models
                .iter()
                .map(|model| (&model.name, model.name_span, &model.fields)),
        )
        .chain(
            schema
                .types
                .iter()
                .map(|ty| (&ty.name, ty.name_span, &ty.fields)),
        );

    for (name, name_span, fields) in owners {
        if span_contains(name_span, offset) {
            return Some(SymbolTarget::Declaration(name.clone()));
        }
        if let Some(field) = fields
            .iter()
            .find(|field| span_contains(field.name_span, offset))
        {
            return Some(SymbolTarget::Field {
                owner: name.clone(),
                field: field.name.clone(),
            });
        }
    }

    for decl in &schema.enums {
        if span_contains(decl.name_span, offset) {
            return Some(SymbolTarget::Declaration(decl.name.clone()));
        }
        if let Some(variant) = decl
            .variants
            .iter()
            .find(|variant| span_contains(variant.span, offset))
        {
            return Some(SymbolTarget::Field {
                owner: decl.name.clone(),
                field: variant.name.clone(),
            });
        }
    }

    if let Some(procedure) = schema
        .procedures
        .iter()
        .find(|procedure| span_contains(procedure.name_span, offset))
    {
        return Some(SymbolTarget::Declaration(procedure.name.clone()));
    }

    crate::query_symbols::declaration_at(schema, offset)
        .map(|name| SymbolTarget::Declaration(name.to_owned()))
}

/// Last resort for positions no span covers: treat the bare word as a global
/// name, but only if the schema actually declares it — otherwise every comment
/// word would report itself as a symbol.
fn word_target(text: &str, schema: &Schema, offset: usize) -> Option<SymbolTarget> {
    let word = word_at_offset(text, offset)?;
    let declared = schema.models.iter().any(|model| model.name == word)
        || schema.types.iter().any(|ty| ty.name == word)
        || schema.mixins.iter().any(|mixin| mixin.name == word)
        || schema.enums.iter().any(|decl| decl.name == word)
        || schema
            .procedures
            .iter()
            .any(|procedure| procedure.name == word)
        || crate::query_symbols::declares(schema, word);
    declared.then(|| SymbolTarget::Declaration(word.to_owned()))
}
