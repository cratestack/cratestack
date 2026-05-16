use std::collections::{BTreeMap, BTreeSet};

use cratestack_core::{Field, Model, Schema};

use crate::diagnostics::{SchemaError, span_error};
use crate::relation_helpers::{parse_relation_attribute, validate_relation_scalar_compatibility};
use crate::validate::fields::{
    CustomFieldSupport, validate_custom_field_attribute, validate_field_policy_attributes,
};
use crate::validate::model_attributes::{validate_model_attributes, validate_model_version_field};
use crate::validate::type_names::validate_type_ref;
use crate::validate::validators::validate_validator_attributes;

pub(super) fn validate_models(
    schema: &Schema,
    type_names: &BTreeSet<String>,
    page_item_type_names: &BTreeSet<String>,
) -> Result<(), SchemaError> {
    let model_names = schema
        .models
        .iter()
        .map(|model| model.name.as_str())
        .collect::<BTreeSet<_>>();

    for model in &schema.models {
        let mut fields = BTreeMap::new();
        let mut has_primary_key = false;
        for field in &model.fields {
            if fields.insert(field.name.clone(), field.span).is_some() {
                return Err(span_error(
                    format!("duplicate field `{}` on model `{}`", field.name, model.name),
                    field.span,
                ));
            }
            if field
                .attributes
                .iter()
                .any(|attribute| attribute.raw.starts_with("@id"))
            {
                has_primary_key = true;
            }
            validate_custom_field_attribute(
                field,
                "model",
                &model.name,
                CustomFieldSupport::Rejected,
            )?;
            validate_type_ref(
                type_names,
                page_item_type_names,
                &field.ty,
                field.span,
                false,
            )?;
            validate_validator_attributes(&model.name, field)?;
            validate_field_policy_attributes(&model.name, field)?;
            validate_field_relation(schema, model, field, &model_names)?;
        }

        validate_model_attributes(model)?;

        if !has_primary_key {
            return Err(span_error(
                format!("model `{}` is missing an @id field", model.name),
                model.span,
            ));
        }

        validate_model_version_field(model)?;
    }
    Ok(())
}

fn validate_field_relation(
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
