//! `@from(Model.field)` on a view field: the shape and the target (ADR 0019
//! D5: an attribute either does something or fails `check`).
//!
//! `@from` is a documented source annotation. Nothing reads its argument,
//! so it was accepted with any text: `@from(!!!not a path)` and
//! `@from(Nope.nothing)` both passed `cratestack check`, and a view whose
//! author mistyped the model or the column kept an annotation that pointed
//! nowhere. It is still not used by a generator, but what it says must now
//! be true: `Model.field`, a model and a field this schema declares.

use cratestack_core::{Field, Schema, View, field_attribute_name};

use crate::diagnostics::{SchemaError, span_error};

fn is_identifier(text: &str) -> bool {
    let mut chars = text.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The text between the parentheses of `@from(...)`.
fn argument(raw: &str) -> &str {
    raw.split_once('(')
        .and_then(|(_, rest)| rest.rsplit_once(')'))
        .map_or("", |(inner, _)| inner)
}

pub(super) fn validate_view_field_from(
    schema: &Schema,
    view: &View,
    field: &Field,
) -> Result<(), SchemaError> {
    for attribute in &field.attributes {
        if field_attribute_name(&attribute.raw) != Some("from") {
            continue;
        }
        let raw = attribute.raw.as_str();
        let owner = format!("view field `{}.{}`", view.name, field.name);
        let Some((model_name, field_name)) = argument(raw)
            .trim()
            .split_once('.')
            .map(|(model, column)| (model.trim(), column.trim()))
            .filter(|(model, column)| is_identifier(model) && is_identifier(column))
        else {
            return Err(span_error(
                format!(
                    "{owner} writes `{raw}`: `@from` names the model column the field is taken \
                     from as `Model.field`, two identifiers joined by one dot, with no quotes \
                     (for example `@from(Customer.email)`). It is refused"
                ),
                attribute.span,
            ));
        };
        let Some(model) = schema.models.iter().find(|model| model.name == model_name) else {
            let models = schema
                .models
                .iter()
                .map(|model| format!("`{}`", model.name))
                .collect::<Vec<_>>()
                .join(", ");
            return Err(span_error(
                format!(
                    "{owner} writes `{raw}`, but `{model_name}` is not a model in this schema \
                     (models: {models}). `@from` names a model column, so the model has to \
                     exist. It is refused"
                ),
                attribute.span,
            ));
        };
        if !model.fields.iter().any(|column| column.name == field_name) {
            let columns = model
                .fields
                .iter()
                .map(|column| format!("`{}`", column.name))
                .collect::<Vec<_>>()
                .join(", ");
            return Err(span_error(
                format!(
                    "{owner} writes `{raw}`, but model `{model_name}` has no field \
                     `{field_name}` (its fields: {columns}). `@from` names a model column, so \
                     the field has to exist. It is refused"
                ),
                attribute.span,
            ));
        }
    }
    Ok(())
}
