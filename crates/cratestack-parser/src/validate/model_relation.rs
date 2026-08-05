//! `@relation(fields:[...],references:[...])` field-level validation,
//! split out of `models.rs` to keep that file under the crate's ~200-LoC
//! convention.

use std::collections::BTreeSet;

use cratestack_core::{Field, Model, Schema};

use crate::diagnostics::{SchemaError, span_error};
use crate::relation_actions::validate_relation_actions;
use crate::relation_helpers::{parse_relation_attribute, validate_relation_scalar_compatibility};

pub(super) fn validate_field_relation(
    schema: &Schema,
    model: &Model,
    field: &Field,
    model_names: &BTreeSet<&str>,
) -> Result<(), SchemaError> {
    let relation_attribute = field
        .attributes
        .iter()
        .find(|attribute| attribute.raw.starts_with("@relation("));
    if model_names.contains(field.ty.name.as_str()) {
        let relation_attribute = relation_attribute.ok_or_else(|| {
            span_error(
                format!(
                    "relation field `{}` on model `{}` must declare @relation(fields:[...],references:[...])",
                    field.name, model.name,
                ),
                field.span,
            )
        })?;
        let relation = parse_relation_attribute(&relation_attribute.raw)
            .map_err(|message| span_error(message, field.span))?;
        if relation.fields.len() != 1 || relation.references.len() != 1 {
            return Err(span_error(
                format!(
                    "relation field `{}` on model `{}` must declare exactly one local field and one reference in this slice",
                    field.name, model.name,
                ),
                field.span,
            ));
        }

        let local_field = model
            .fields
            .iter()
            .find(|candidate| candidate.name == relation.fields[0])
            .ok_or_else(|| {
                span_error(
                    format!(
                        "relation field `{}` on model `{}` references unknown local field `{}`",
                        field.name, model.name, relation.fields[0],
                    ),
                    field.span,
                )
            })?;
        if model_names.contains(local_field.ty.name.as_str()) {
            return Err(span_error(
                format!(
                    "relation field `{}` on model `{}` must use a scalar local field, found relation field `{}`",
                    field.name, model.name, local_field.name,
                ),
                field.span,
            ));
        }

        let target_model = schema
            .models
            .iter()
            .find(|candidate| candidate.name == field.ty.name)
            .ok_or_else(|| {
                span_error(
                    format!(
                        "relation field `{}` on model `{}` references unknown target model `{}`",
                        field.name, model.name, field.ty.name,
                    ),
                    field.span,
                )
            })?;
        let target_field = target_model
            .fields
            .iter()
            .find(|candidate| candidate.name == relation.references[0])
            .ok_or_else(|| {
                span_error(
                    format!(
                        "relation field `{}` on model `{}` references unknown target field `{}` on `{}`",
                        field.name, model.name, relation.references[0], target_model.name,
                    ),
                    field.span,
                )
            })?;
        if model_names.contains(target_field.ty.name.as_str()) {
            return Err(span_error(
                format!(
                    "relation field `{}` on model `{}` must reference a scalar target field, found relation field `{}`",
                    field.name, model.name, target_field.name,
                ),
                field.span,
            ));
        }
        validate_relation_scalar_compatibility(field, model, local_field, target_field)?;
        validate_relation_actions(field, model, local_field, &relation)?;
    } else if relation_attribute.is_some() {
        return Err(span_error(
            format!(
                "scalar field `{}` on model `{}` cannot declare @relation(...)",
                field.name, model.name,
            ),
            field.span,
        ));
    }
    Ok(())
}
