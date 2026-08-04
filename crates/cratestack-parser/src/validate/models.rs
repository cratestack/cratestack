use std::collections::{BTreeMap, BTreeSet};

use cratestack_core::{Field, Model, Schema};

use crate::diagnostics::{SchemaError, span_error};
use crate::relation_actions::validate_relation_actions;
use crate::relation_helpers::{parse_relation_attribute, validate_relation_scalar_compatibility};
use crate::validate::fields::{
    CustomFieldSupport, validate_custom_field_attribute, validate_default_dbgenerated_no_args,
    validate_field_list_arity_support, validate_field_policy_attributes,
    validate_field_reserved_identifier,
};
use crate::validate::model_attributes::{validate_model_attributes, validate_model_version_field};
use crate::validate::pb::validate_pb_field_attribute;
use crate::validate::type_names::{
    collect_type_decl_names, reject_type_decl_as_model_field_type, validate_type_ref,
};
use crate::validate::validators::validate_validator_attributes;

pub(super) fn validate_models(
    schema: &Schema,
    type_names: &BTreeSet<String>,
    page_item_type_names: &BTreeSet<String>,
    find_many_model_names: &BTreeSet<String>,
) -> Result<(), SchemaError> {
    let model_names = schema
        .models
        .iter()
        .map(|model| model.name.as_str())
        .collect::<BTreeSet<_>>();
    let schema_has_datasource = schema.datasource.is_some();

    // See `type_names::reject_type_decl_as_model_field_type` (#230): a
    // `type` block cannot back a model field's storage column.
    let type_decl_names = collect_type_decl_names(schema);

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
            validate_field_reserved_identifier(field, "model", &model.name)?;
            validate_type_ref(
                type_names,
                page_item_type_names,
                find_many_model_names,
                &schema.declared_extensions,
                &field.ty,
                field.span,
                crate::validate::type_names::TypeRefAllow {
                    vector: true,
                    ..Default::default()
                },
            )?;
            reject_type_decl_as_model_field_type(&type_decl_names, &model.name, field)?;
            validate_validator_attributes(&model.name, field)?;
            validate_field_policy_attributes(&model.name, field)?;
            validate_default_dbgenerated_no_args(&model.name, field)?;
            validate_pb_field_attribute("model", &model.name, field)?;
            validate_field_list_arity_support(
                schema_has_datasource,
                &model.name,
                &model_names,
                field,
            )?;
            validate_field_relation(schema, model, field, &model_names)?;
        }

        validate_model_attributes(model, &model_names)?;

        if !has_primary_key {
            has_primary_key = model.attributes.iter().any(|a| a.raw.starts_with("@@id("));
        }

        if !has_primary_key {
            return Err(span_error(
                format!(
                    "model `{}` is missing an @id field (or a model-level @@id([...]) composite key)",
                    model.name
                ),
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
