use std::collections::BTreeMap;
use std::collections::BTreeSet;

use cratestack_core::Schema;

use crate::diagnostics::{SchemaError, span_error};
use crate::validate::fields::{
    CustomFieldSupport, validate_custom_field_attribute, validate_default_dbgenerated_no_args,
    validate_field_list_arity_support, validate_field_policy_attributes,
    validate_field_reserved_identifier,
};
use crate::validate::model_attributes::{validate_model_attributes, validate_model_version_field};
use crate::validate::model_relation::validate_field_relation;
use crate::validate::pb::validate_pb_field_attribute;
use crate::validate::reserved_idents::validate_reserved_identifier;
use crate::validate::snake_case_collisions::{
    validate_field_column_collisions, validate_model_name_collisions,
};
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

    validate_model_name_collisions(&schema.models)?;

    for model in &schema.models {
        validate_reserved_identifier(
            &model.name,
            model.name_span,
            &format!("model `{}`", model.name),
        )?;
        validate_field_column_collisions(&model.fields, "model", &model.name)?;

        let mut fields = BTreeMap::new();
        let mut has_primary_key = false;
        // cratestack#536: two field-level `@id` attributes on one model is
        // a multi-column primary key by another spelling. `@@id([a, b])`
        // is hard-rejected at macro expansion citing #136
        // (`reject_composite_primary_keys` in
        // `cratestack-macros/src/include/parse.rs`) because codegen (query
        // builders, routing, generated clients) still assumes exactly one
        // scalar `@id` — but nothing stopped this equivalent form from
        // reaching `cratestack-migrate`, which marks every `@id`-tagged
        // column `primary_key = true` and happily emits a real
        // multi-column `PRIMARY KEY`. Track the first `@id` field so a
        // second one is rejected with the same #136 reasoning, keeping
        // both spellings consistent.
        let mut first_id_field: Option<&str> = None;
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
                if let Some(first) = first_id_field {
                    return Err(span_error(
                        format!(
                            "model `{}` declares more than one field-level `@id` (`{}` and `{}`), \
                             which is a multi-column primary key — the same restriction as \
                             `@@id([...])`, not yet supported by codegen (query builders, routing, \
                             and generated clients still assume a single scalar `@id`); see \
                             https://github.com/cratestack/cratestack/issues/136 for status. Use \
                             exactly one `@id` field.",
                            model.name, first, field.name,
                        ),
                        field.span,
                    ));
                }
                first_id_field = Some(field.name.as_str());
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

        validate_model_attributes(model, &model_names, schema.transport)?;

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
