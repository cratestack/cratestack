use cratestack_core::{Model, parse_emit_attribute};

use crate::diagnostics::{SchemaError, span_error};

pub(super) fn validate_model_attributes(model: &Model) -> Result<(), SchemaError> {
    let mut saw_emit_attribute = false;
    let mut saw_paged_attribute = false;
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
