//! Where, and how, `@server_only` may be written.
//!
//! `@server_only` keeps a *stored scalar column of a model* out of every
//! generated input and output shape. Anywhere else it has no effect, and an
//! attribute that parses but does nothing reads as a guarantee the schema
//! does not give — so each such placement is refused here:
//!
//! 1. on a field of a `type` block (not a model);
//! 2. on a relation field (a field whose type is another model);
//! 3. on a scalar used as a relation key — named in any `@relation`'s
//!    `fields: [...]` on its own model, or in `references: [...]` of a
//!    relation on another model that targets its model;
//! 4. together with `@version` on the same field;
//! 5. on a field of the `auth` block.
//!
//! Every generator recognises the attribute only when its raw text is
//! exactly `@server_only`, so any other spelling that still parses —
//! `@server_only()`, `@server_only(true)`, or `@server_only@unique` with the
//! separating space missing — is inert on every field, including a plain
//! model scalar. `super::attribute_spelling` refuses those, together with
//! the same spellings of every other attribute that takes no arguments, for
//! every kind of field-bearing block. The rules here match only the exact
//! spelling; every other one is refused there.
//!
//! The primary-key and `@readonly` combinations are refused separately, in
//! [`super::fields::validate_field_policy_attributes`].

use std::collections::BTreeSet;

use cratestack_core::{Attribute, Field, Model, Schema};

use crate::diagnostics::{SchemaError, span_error};
use crate::relation_helpers::parse_relation_attribute;

const SERVER_ONLY: &str = "@server_only";

/// The field's `@server_only` attribute, spelled exactly as the generators
/// recognise it. The diagnostics below point at its span: it is the text to
/// remove.
fn server_only_attribute(field: &Field) -> Option<&Attribute> {
    field.attributes.iter().find(|a| a.raw == SERVER_ONLY)
}

/// The first relation (as `Model.field`) that joins on `model.field`, if any.
///
/// Both ends of every relation declaration count: the local `fields` of the
/// declaring model and the `references` of its target model. So a key is
/// found whichever side the relation is declared on — a to-one on the
/// foreign-key side names the key in `fields`, a to-many on the other side
/// names the same column in `references` — and on a self-relation both ends
/// are the same model. Only the first `@relation(...)` of a field counts,
/// as in `validate_field_relation` and in codegen. Malformed attributes are
/// skipped: `validate_field_relation` reports those.
fn relation_joining_on(
    schema: &Schema,
    model_names: &BTreeSet<&str>,
    model: &str,
    field: &str,
) -> Option<String> {
    for owner in &schema.models {
        for relation_field in &owner.fields {
            if !model_names.contains(relation_field.ty.name.as_str()) {
                continue;
            }
            let Some(relation) = relation_field
                .attributes
                .iter()
                .find(|a| a.raw.starts_with("@relation("))
                .and_then(|a| parse_relation_attribute(&a.raw).ok())
            else {
                continue;
            };
            let local = owner.name == model && relation.fields.iter().any(|f| f == field);
            let target =
                relation_field.ty.name == model && relation.references.iter().any(|f| f == field);
            if local || target {
                return Some(format!("{}.{}", owner.name, relation_field.name));
            }
        }
    }
    None
}

/// Rules 2–4, for one field of a model (mixin fields are checked here too,
/// once expanded into the model that uses them).
pub(super) fn validate_model_field(
    schema: &Schema,
    model: &Model,
    field: &Field,
    model_names: &BTreeSet<&str>,
) -> Result<(), SchemaError> {
    let Some(attribute) = server_only_attribute(field) else {
        return Ok(());
    };
    let model_name = model.name.as_str();
    let name = format!("{model_name}.{}", field.name);
    if model_names.contains(field.ty.name.as_str()) {
        return Err(span_error(
            format!(
                "relation field `{name}` declares @server_only; `@server_only` has no effect on a \
                 relation field (it applies to stored scalar columns only), so it is refused. \
                 Remove it, and mark the individual fields of `{}` that must stay server-side \
                 with `@server_only` instead",
                field.ty.name,
            ),
            attribute.span,
        ));
    }
    if let Some(via) = relation_joining_on(schema, model_names, model_name, &field.name) {
        return Err(span_error(
            format!(
                "field `{name}` declares @server_only but is a key of relation `{via}`; a \
                 relation key cannot be kept server-side (its value travels with the relation: \
                 it is the foreign-key column on one side and the referenced column on the \
                 other), so `@server_only` is refused. Remove it; to stop clients from setting \
                 the key, mark it @readonly instead",
            ),
            attribute.span,
        ));
    }
    if field.attributes.iter().any(|a| a.raw == "@version") {
        return Err(span_error(
            format!(
                "field `{name}` declares both @version and @server_only; `@server_only` has no \
                 effect on a @version field (clients need the version for conditional writes), \
                 so it is refused. Remove @server_only",
            ),
            attribute.span,
        ));
    }
    Ok(())
}

/// Rule 1, for one field of a `type` block.
pub(super) fn validate_type_field(type_name: &str, field: &Field) -> Result<(), SchemaError> {
    let Some(attribute) = server_only_attribute(field) else {
        return Ok(());
    };
    Err(span_error(
        format!(
            "field `{type_name}.{}` declares @server_only, but `{type_name}` is a `type`, not a \
             `model`; `@server_only` has no effect on a `type` field (it applies to stored model \
             columns only), so it is refused. Remove it, and leave the value out of the type if \
             it must not be sent",
            field.name,
        ),
        attribute.span,
    ))
}

/// Rule 5, for one field of the `auth` block.
pub(super) fn validate_auth_field(auth_name: &str, field: &Field) -> Result<(), SchemaError> {
    let Some(attribute) = server_only_attribute(field) else {
        return Ok(());
    };
    Err(span_error(
        format!(
            "field `{auth_name}.{}` declares @server_only, but `{auth_name}` is the `auth` block, \
             not a `model`; `@server_only` has no effect on an auth field (it applies to stored \
             model columns only, and no generator reads the attributes of an auth field), so it \
             is refused. Remove it",
            field.name,
        ),
        attribute.span,
    ))
}
