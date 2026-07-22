use std::collections::BTreeSet;

use cratestack_core::{Model, parse_composite_id_attribute, parse_emit_attribute};

use crate::diagnostics::{SchemaError, span_error};

pub(super) fn validate_model_attributes(
    model: &Model,
    model_names: &BTreeSet<&str>,
) -> Result<(), SchemaError> {
    let mut saw_emit_attribute = false;
    let mut saw_paged_attribute = false;
    let mut saw_id_attribute = false;
    for attribute in &model.attributes {
        if attribute.raw.starts_with("@@emit(") {
            if saw_emit_attribute {
                return Err(span_error(
                    format!(
                        "model `{}` must not declare more than one @@emit(...) attribute",
                        model.name
                    ),
                    attribute.span,
                ));
            }
            parse_emit_attribute(&attribute.raw)
                .map_err(|message| span_error(message, attribute.span))?;
            saw_emit_attribute = true;
        } else if attribute.raw.starts_with("@@paged") {
            if attribute.raw != "@@paged" {
                return Err(span_error(
                    format!(
                        "model `{}` uses unsupported paging directive `{}`; use bare `@@paged` in this slice",
                        model.name, attribute.raw,
                    ),
                    attribute.span,
                ));
            }
            if saw_paged_attribute {
                return Err(span_error(
                    format!(
                        "model `{}` must not declare more than one @@paged attribute",
                        model.name
                    ),
                    attribute.span,
                ));
            }
            saw_paged_attribute = true;
        } else if attribute.raw == "@@audit" {
            // recognised; no further validation needed at parse time
        } else if attribute.raw.starts_with("@@audit(") {
            return Err(span_error(
                format!(
                    "model `{}` `@@audit` does not take arguments; use bare `@@audit`",
                    model.name,
                ),
                attribute.span,
            ));
        } else if attribute.raw == "@@soft_delete" {
            // recognised; descriptor wiring lives in the macro
        } else if attribute.raw.starts_with("@@soft_delete(") {
            return Err(span_error(
                format!(
                    "model `{}` `@@soft_delete` does not take arguments",
                    model.name,
                ),
                attribute.span,
            ));
        } else if attribute.raw.starts_with("@@retain(") {
            validate_retain_attribute(model, attribute)?;
        } else if attribute.raw.starts_with("@@id(") {
            if saw_id_attribute {
                return Err(span_error(
                    format!(
                        "model `{}` must not declare more than one @@id(...) attribute",
                        model.name
                    ),
                    attribute.span,
                ));
            }
            validate_composite_id_attribute(model, attribute, model_names)?;
            saw_id_attribute = true;
        }
    }
    Ok(())
}

/// Validates a `@@id([field1, field2, ...])` composite-primary-key
/// attribute: syntax, mutual exclusivity with a field-level `@id`, and
/// that every listed field is a real scalar field on this model.
fn validate_composite_id_attribute(
    model: &Model,
    attribute: &cratestack_core::Attribute,
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
        let field = model
            .fields
            .iter()
            .find(|candidate| &candidate.name == field_name)
            .ok_or_else(|| {
                span_error(
                    format!(
                        "model `{}` `@@id([...])` references unknown field `{}`",
                        model.name, field_name,
                    ),
                    attribute.span,
                )
            })?;

        if model_names.contains(field.ty.name.as_str()) {
            return Err(span_error(
                format!(
                    "model `{}` `@@id([...])` field `{}` must be a scalar column, not a relation field",
                    model.name, field_name,
                ),
                attribute.span,
            ));
        }

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

fn validate_retain_attribute(
    model: &Model,
    attribute: &cratestack_core::Attribute,
) -> Result<(), SchemaError> {
    let inner = attribute
        .raw
        .strip_prefix("@@retain(")
        .and_then(|s| s.strip_suffix(')'))
        .ok_or_else(|| {
            span_error(
                format!("model `{}` `@@retain` is malformed", model.name),
                attribute.span,
            )
        })?
        .trim();
    let days_str = inner.strip_prefix("days:").map(str::trim).ok_or_else(|| {
        span_error(
            format!("model `{}` `@@retain` requires `days: N`", model.name,),
            attribute.span,
        )
    })?;
    days_str.parse::<u32>().map_err(|_| {
        span_error(
            format!(
                "model `{}` `@@retain(days: ...)` must be a non-negative integer",
                model.name,
            ),
            attribute.span,
        )
    })?;
    Ok(())
}

pub(super) fn validate_model_version_field(model: &Model) -> Result<(), SchemaError> {
    let version_fields: Vec<&cratestack_core::Field> = model
        .fields
        .iter()
        .filter(|field| field.attributes.iter().any(|a| a.raw == "@version"))
        .collect();
    if version_fields.len() > 1 {
        return Err(span_error(
            format!(
                "model `{}` declares more than one @version field",
                model.name,
            ),
            version_fields[1].span,
        ));
    }
    if let Some(version) = version_fields.first() {
        if version.ty.name != "Int"
            || !matches!(version.ty.arity, cratestack_core::TypeArity::Required)
        {
            return Err(span_error(
                format!(
                    "@version field `{}.{}` must be a required `Int`",
                    model.name, version.name,
                ),
                version.span,
            ));
        }
        if version
            .attributes
            .iter()
            .any(|attribute| attribute.raw.starts_with("@id"))
        {
            return Err(span_error(
                format!(
                    "@version field `{}.{}` must not also be the primary key",
                    model.name, version.name,
                ),
                version.span,
            ));
        }
    }
    Ok(())
}
