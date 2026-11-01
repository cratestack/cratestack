//! `subscription procedure` and `topic` declarations
//! (`docs/design/subscriptions-websocket.md` §3.2, §3.4, slice S4).
//!
//! A subscription procedure is checked as a procedure is (identifiers,
//! argument and return types, `@allow`/`@deny`/`@authn`/`@authorize`/
//! `@api_version`), plus what makes it a subscription: a single item type,
//! and none of the attributes that describe a unary call. A topic's key is
//! a scalar a channel name can carry; its payload is any declared type.

use std::collections::BTreeSet;

use cratestack_core::{Procedure, Schema, Topic, TypeArity};

use super::attribute_shape::{Arguments, Known, check_shape};
use super::procedure_idents::validate_procedure_idents;
use super::procedures::validate_procedure_api_version_attribute;
use super::reserved_idents::validate_reserved_identifier;
use super::type_names::{TypeRefAllow, validate_type_ref};
use crate::diagnostics::{SchemaError, span_error};

/// What each refused attribute means on a unary call, and why a
/// subscription has no use for it.
const REFUSED: &[(&str, &str)] = &[
    ("@stream", "a subscription is already a stream"),
    (
        "@isolation",
        "a subscription holds no transaction open for its lifetime",
    ),
    ("@no_idempotency", "a subscription has no replayable result"),
    (
        "@status",
        "a subscription answers with a stream, not a status",
    ),
    (
        "@no_rate_limit",
        "a subscription opens once, and is not rate limited per item",
    ),
    ("@mcp", "MCP exposes only unary tools"),
];

const ATTRIBUTES: &[Known] = &[
    ("@allow", Arguments::Required),
    ("@deny", Arguments::Required),
    ("@authn", Arguments::Required),
    ("@authorize", Arguments::Required),
    ("@api_version", Arguments::Required),
];

/// Scalars a topic key may be: each renders as the channel's last segment.
const TOPIC_KEY_TYPES: &[&str] = &["String", "Int", "Uuid"];

pub(super) fn validate_subscriptions(
    schema: &Schema,
    type_names: &BTreeSet<String>,
    page_item_type_names: &BTreeSet<String>,
    model_names: &BTreeSet<String>,
) -> Result<(), SchemaError> {
    let procedure_names: BTreeSet<&str> =
        schema.procedures.iter().map(|p| p.name.as_str()).collect();
    let mut seen = BTreeSet::new();
    for procedure in &schema.subscription_procedures {
        if procedure_names.contains(procedure.name.as_str()) || !seen.insert(&procedure.name) {
            return Err(span_error(
                format!("duplicate procedure name `{}`", procedure.name),
                procedure.span,
            ));
        }
        validate_procedure_idents(procedure)?;
        let check = |ty, allow| {
            validate_type_ref(
                type_names,
                page_item_type_names,
                model_names,
                &schema.declared_extensions,
                ty,
                procedure.span,
                allow,
            )
        };
        for arg in &procedure.args {
            check(&arg.ty, TypeRefAllow::default())?;
        }
        check(&procedure.return_type, TypeRefAllow::default())?;
        if procedure.return_type.arity != TypeArity::Required {
            return Err(span_error(
                format!(
                    "subscription procedure `{}` must return a single item type, such as `{}`: \
                     each item of the stream is one value",
                    procedure.name, procedure.return_type.name,
                ),
                procedure.return_type.name_span,
            ));
        }
        validate_attributes(procedure)?;
        validate_procedure_api_version_attribute(procedure)?;
        super::authn_attribute::validate_procedure_authn_attribute(procedure)?;
    }
    let mut topics = BTreeSet::new();
    for topic in &schema.topics {
        if !topics.insert(&topic.name) {
            return Err(span_error(
                format!("duplicate topic name `{}`", topic.name),
                topic.span,
            ));
        }
        validate_topic(schema, topic, type_names, page_item_type_names, model_names)?;
    }
    Ok(())
}

fn validate_attributes(procedure: &Procedure) -> Result<(), SchemaError> {
    let owner = format!("subscription procedure `{}`", procedure.name);
    for attribute in &procedure.attributes {
        if let Some((name, why)) = REFUSED.iter().find(|(name, _)| {
            attribute.raw == *name || attribute.raw.starts_with(&format!("{name}("))
        }) {
            return Err(span_error(
                format!("{owner} cannot declare `{name}`: {why}"),
                attribute.span,
            ));
        }
        check_shape(attribute, ATTRIBUTES, &owner, "subscription procedure")?;
    }
    Ok(())
}

fn validate_topic(
    schema: &Schema,
    topic: &Topic,
    type_names: &BTreeSet<String>,
    page_item_type_names: &BTreeSet<String>,
    model_names: &BTreeSet<String>,
) -> Result<(), SchemaError> {
    let owner = format!("topic `{}`", topic.name);
    validate_reserved_identifier(&topic.name, topic.name_span, &owner)?;
    let key = &topic.key;
    if key.ty.arity != TypeArity::Required || !TOPIC_KEY_TYPES.contains(&key.ty.name.as_str()) {
        return Err(span_error(
            format!(
                "{owner}'s key `{}` must be one of {}: it names the channel",
                key.name,
                TOPIC_KEY_TYPES.join(", "),
            ),
            key.span,
        ));
    }
    validate_type_ref(
        type_names,
        page_item_type_names,
        model_names,
        &schema.declared_extensions,
        &topic.payload,
        topic.span,
        TypeRefAllow::default(),
    )?;
    if topic.payload.arity != TypeArity::Required {
        return Err(span_error(
            format!(
                "{owner}'s payload must be a single type, such as `{}`",
                topic.payload.name
            ),
            topic.payload.name_span,
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
