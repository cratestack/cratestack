use std::collections::{BTreeMap, BTreeSet};

use cratestack_core::Schema;

use crate::diagnostics::{SchemaError, span_error};
use crate::validate::fields::{
    CustomFieldSupport, validate_custom_field_attribute, validate_default_dbgenerated_no_args,
};
use crate::validate::type_names::validate_type_ref;

pub(super) fn validate_mixins(
    schema: &Schema,
    type_names: &BTreeSet<String>,
    page_item_type_names: &BTreeSet<String>,
) -> Result<(), SchemaError> {
    for mixin in &schema.mixins {
        let mut fields = BTreeMap::new();
        for field in &mixin.fields {
            if fields.insert(field.name.clone(), field.span).is_some() {
                return Err(span_error(
                    format!("duplicate field `{}` on mixin `{}`", field.name, mixin.name),
                    field.span,
                ));
            }
            if field
                .attributes
                .iter()
                .any(|attribute| attribute.raw.starts_with("@id"))
            {
                return Err(span_error(
                    format!(
                        "field `{}` on mixin `{}` cannot declare @id",
                        field.name, mixin.name
                    ),
                    field.span,
                ));
            }
            if field
                .attributes
                .iter()
                .any(|attribute| attribute.raw.starts_with("@@"))
            {
                return Err(span_error(
                    format!(
                        "field `{}` on mixin `{}` cannot declare model-level attributes",
                        field.name, mixin.name
                    ),
                    field.span,
                ));
            }
            validate_custom_field_attribute(
                field,
                "mixin",
                &mixin.name,
                CustomFieldSupport::Rejected,
            )?;
            validate_type_ref(
                type_names,
                page_item_type_names,
                &field.ty,
                field.span,
                false,
            )?;
            validate_default_dbgenerated_no_args(&mixin.name, field)?;
        }
    }
    Ok(())
}

pub(super) fn validate_types(
    schema: &Schema,
    type_names: &BTreeSet<String>,
    page_item_type_names: &BTreeSet<String>,
) -> Result<(), SchemaError> {
    for ty in &schema.types {
        let mut fields = BTreeSet::new();
        for field in &ty.fields {
            if !fields.insert(field.name.clone()) {
                return Err(span_error(
                    format!("duplicate field `{}` on type `{}`", field.name, ty.name),
                    field.span,
                ));
            }
            validate_custom_field_attribute(field, "type", &ty.name, CustomFieldSupport::TypeOnly)?;
            validate_type_ref(
                type_names,
                page_item_type_names,
                &field.ty,
                field.span,
                false,
            )?;
        }
    }
    Ok(())
}

pub(super) fn validate_enums(schema: &Schema) -> Result<(), SchemaError> {
    for enum_decl in &schema.enums {
        let mut variants = BTreeSet::new();
        for variant in &enum_decl.variants {
            if !variants.insert(variant.name.clone()) {
                return Err(span_error(
                    format!(
                        "duplicate variant `{}` on enum `{}`",
                        variant.name, enum_decl.name
                    ),
                    variant.span,
                ));
            }
        }
    }
    Ok(())
}

pub(super) fn validate_auth(
    schema: &Schema,
    type_names: &BTreeSet<String>,
    page_item_type_names: &BTreeSet<String>,
) -> Result<(), SchemaError> {
    if let Some(auth) = &schema.auth {
        let mut fields = BTreeSet::new();
        for field in &auth.fields {
            if !fields.insert(field.name.clone()) {
                return Err(span_error(
                    format!(
                        "duplicate field `{}` on auth block `{}`",
                        field.name, auth.name
                    ),
                    field.span,
                ));
            }
            validate_custom_field_attribute(
                field,
                "auth block",
                &auth.name,
                CustomFieldSupport::Rejected,
            )?;
            validate_type_ref(
                type_names,
                page_item_type_names,
                &field.ty,
                field.span,
                false,
            )?;
        }
    }
    Ok(())
}
