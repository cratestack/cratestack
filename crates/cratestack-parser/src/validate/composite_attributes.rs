//! Semantic checks for the two model-level attributes that take a
//! bracketed list of local fields: `@@id([...])` (composite primary
//! key) and `@@unique([...])` (composite unique constraint).
//!
//! Both resolve every listed name against the model's real scalar
//! fields, so a typo or a relation field is a schema error at
//! `cratestack check` time rather than a constraint that silently
//! never reaches the database (issue #262).

use std::collections::BTreeSet;

use cratestack_core::parse_composite_unique_attribute;
use cratestack_core::{Attribute, Model, parse_composite_id_attribute};

use crate::diagnostics::{SchemaError, span_error};

/// Validates a `@@id([field1, field2, ...])` composite-primary-key
/// attribute: syntax, mutual exclusivity with a field-level `@id`, and
/// that every listed field is a real scalar field on this model.
pub(super) fn validate_composite_id_attribute(
    model: &Model,
    attribute: &Attribute,
    model_names: &BTreeSet<&str>,
) -> Result<(), SchemaError> {
    let field_names = parse_composite_id_attribute(&attribute.raw)
        .map_err(|message| span_error(message, attribute.span))?;

    if let Some(single_id_field) = model
        .fields
        .iter()
        .find(|field| field.attributes.iter().any(|a| a.raw.starts_with("@id")))
    {
        return Err(span_error(
            format!(
                "model `{}` declares both a field-level `@id` on `{}` and `@@id([...])`; use exactly one primary key declaration",
                model.name, single_id_field.name,
            ),
            attribute.span,
        ));
    }

    for field_name in &field_names {
        let field = resolve_scalar_field(model, attribute, model_names, field_name, "@@id([...])")?;

        if field
            .attributes
            .iter()
            .any(|a| a.raw == "@readonly" || a.raw == "@server_only")
        {
            return Err(span_error(
                format!(
                    "model `{}` `@@id([...])` field `{}` is part of the primary key and must not declare @readonly or @server_only",
                    model.name, field_name,
                ),
                attribute.span,
            ));
        }

        if field.attributes.iter().any(|a| a.raw == "@version") {
            return Err(span_error(
                format!(
                    "model `{}` `@@id([...])` field `{}` must not also be the @version field",
                    model.name, field_name,
                ),
                attribute.span,
            ));
        }
    }

    Ok(())
}

/// Validates a `@@unique([field1, field2, ...])` composite-unique
/// attribute. A model may declare several of them — each becomes its
/// own `CREATE UNIQUE INDEX` — but two attributes listing the same
/// fields in the same order would collide on the generated index name,
/// so that is rejected here.
pub(super) fn validate_composite_unique_attribute(
    model: &Model,
    attribute: &Attribute,
    model_names: &BTreeSet<&str>,
    seen: &mut Vec<Vec<String>>,
) -> Result<(), SchemaError> {
    if !attribute.raw.starts_with("@@unique(") {
        return Err(span_error(
            format!(
                "model `{}` `@@unique` requires a field list: `@@unique([field1, field2])`",
                model.name,
            ),
            attribute.span,
        ));
    }

    let field_names = parse_composite_unique_attribute(&attribute.raw)
        .map_err(|message| span_error(message, attribute.span))?;

    for field_name in &field_names {
        resolve_scalar_field(model, attribute, model_names, field_name, "@@unique([...])")?;
    }

    if seen.contains(&field_names) {
        return Err(span_error(
            format!(
                "model `{}` declares the same `@@unique([{}])` constraint more than once",
                model.name,
                field_names.join(", "),
            ),
            attribute.span,
        ));
    }
    seen.push(field_names);

    Ok(())
}

/// Resolves one listed field name to a scalar field on `model`, or
/// fails with an error naming the attribute that referenced it.
///
/// `pub(super)` — also used by [`super::index_attribute`] to validate
/// `@@index([...])`'s field list with the exact same rule.
pub(super) fn resolve_scalar_field<'model>(
    model: &'model Model,
    attribute: &Attribute,
    model_names: &BTreeSet<&str>,
    field_name: &str,
    attribute_label: &str,
) -> Result<&'model cratestack_core::Field, SchemaError> {
    let field = model
        .fields
        .iter()
        .find(|candidate| candidate.name == field_name)
        .ok_or_else(|| {
            span_error(
                format!(
                    "model `{}` `{attribute_label}` references unknown field `{field_name}`",
                    model.name,
                ),
                attribute.span,
            )
        })?;

    if model_names.contains(field.ty.name.as_str()) {
        return Err(span_error(
            format!(
                "model `{}` `{attribute_label}` field `{field_name}` must be a scalar column, not a relation field",
                model.name,
            ),
            attribute.span,
        ));
    }

    if field.attributes.iter().any(|a| a.raw == "@computed") {
        return Err(span_error(
            format!(
                "model `{}` `{attribute_label}` field `{field_name}` is `@computed` — computed \
                 fields are resolved at response time, never stored, so they cannot participate \
                 in database keys, constraints, or indexes",
                model.name,
            ),
            attribute.span,
        ));
    }

    Ok(field)
}
