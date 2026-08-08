use std::collections::BTreeSet;

use cratestack_core::{Model, TransportStyle, parse_emit_attribute};

use crate::diagnostics::{SchemaError, span_error};

use super::composite_attributes::{
    validate_composite_id_attribute, validate_composite_unique_attribute,
};
use super::index_attribute::{SeenIndexAttributes, validate_index_attribute};

pub(super) fn validate_model_attributes(
    model: &Model,
    model_names: &BTreeSet<&str>,
    transport: TransportStyle,
) -> Result<(), SchemaError> {
    let mut saw_emit_attribute = false;
    let mut saw_paged_attribute = false;
    let mut saw_id_attribute = false;
    let mut subscribe_attribute: Option<&cratestack_core::Attribute> = None;
    // Every `@@unique([...])` list seen so far on this model, so a
    // repeated constraint (which would collide on the generated index
    // name) is caught rather than emitted twice.
    let mut unique_field_lists: Vec<Vec<String>> = Vec::new();
    // Every `@@index([...])` (fields, using) pair seen so far — see
    // `SeenIndexAttributes`'s doc for why `using` is part of the key.
    let mut index_attributes: SeenIndexAttributes = Vec::new();
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
        } else if attribute.raw == "@@subscribe" {
            if !matches!(transport, TransportStyle::Rpc) {
                return Err(span_error(
                    format!(
                        "model `{}` declares `@@subscribe`, which requires the schema to \
                         declare `transport rpc` — subscriptions are only dispatched via \
                         `GET /rpc/subscribe/{{op_id}}` (docs/design/rpc-transport.md §3.4a)",
                        model.name,
                    ),
                    attribute.span,
                ));
            }
            subscribe_attribute = Some(attribute);
        } else if attribute.raw.starts_with("@@subscribe(") {
            return Err(span_error(
                format!(
                    "model `{}` `@@subscribe` does not take arguments; use bare `@@subscribe`",
                    model.name,
                ),
                attribute.span,
            ));
        } else if attribute.raw.starts_with("@@internal") {
            // SPIKE (`spike/b1-internal-actions`): declares that an
            // action has a policy but generates no REST route. Purely
            // a codegen marker — it does not touch policy evaluation,
            // so there is nothing to cross-check against the model's
            // `@@allow` rules here.
            cratestack_core::parse_internal_attribute(&attribute.raw).map_err(|message| {
                span_error(format!("model `{}`: {message}", model.name), attribute.span)
            })?;
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
        } else if attribute.raw == "@@unique" || attribute.raw.starts_with("@@unique(") {
            validate_composite_unique_attribute(
                model,
                attribute,
                model_names,
                &mut unique_field_lists,
            )?;
        } else if attribute.raw == "@@index" || attribute.raw.starts_with("@@index(") {
            validate_index_attribute(model, attribute, model_names, &mut index_attributes)?;
        }
    }

    // A subscription with nothing to subscribe to is a footgun, not a
    // valid empty state: without `@@emit(...)` no model event is ever
    // enqueued, so `GET /rpc/subscribe/model.<X>.subscribe` would
    // silently connect and never deliver anything, forever. Fail at
    // parse time instead — see docs/design/rpc-transport.md §3.4a.
    if let Some(subscribe_attribute) = subscribe_attribute
        && !saw_emit_attribute
    {
        return Err(span_error(
            format!(
                "model `{}` declares `@@subscribe` but no `@@emit(...)`; a subscription needs \
                 at least one emitted operation to stream (`@@emit(created, updated, deleted)`)",
                model.name,
            ),
            subscribe_attribute.span,
        ));
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
