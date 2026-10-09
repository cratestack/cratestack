//! A validator on a `type` field that no client input can reach (ADR 0019
//! D5: an attribute either does something or fails `check`).
//!
//! A validator on a `type` field runs on a value a client sent: a procedure
//! argument, or the params of a `@computed` field in `?computedParams=`,
//! reached directly or nested through other `type`s and `model`s
//! (`cratestack-macros/src/validators/types.rs`). A `type` that only a
//! procedure returns, or that only a server-side `@computed` field takes
//! (no params travel on a procedure output, `computed/compose.rs`), is built
//! by the server, so its validators never run: `@length` on a return-only
//! `type` passed `cratestack check` and the invalid value was returned with
//! `200`. A `type` used both as an argument and as a return is reachable and
//! keeps its validators.

use std::collections::{BTreeMap, BTreeSet};

use cratestack_core::{
    Field, Schema, TypeDecl, computed_params_type_name, field_attribute_name, is_computed_field,
};

use crate::diagnostics::{SchemaError, span_error};

const VALIDATORS: [&str; 6] = ["length", "range", "regex", "email", "uri", "iso4217"];

/// The names a client's value can be, or contain: what the procedures take
/// as arguments and what a model's `?computedParams=` decodes into, and
/// every `type` those hold. A `model` holds only scalars in a value a client
/// sends (a relation is another row), so it ends the walk.
fn client_reachable(schema: &Schema) -> BTreeSet<&str> {
    let types: BTreeMap<&str, &TypeDecl> = schema
        .types
        .iter()
        .map(|ty| (ty.name.as_str(), ty))
        .collect();
    let mut pending: Vec<&str> = schema
        .procedures
        .iter()
        .flat_map(|procedure| procedure.args.iter())
        .map(|arg| arg.ty.name.as_str())
        .chain(
            schema
                .models
                .iter()
                .flat_map(|model| model.fields.iter())
                .filter_map(computed_params_type_name),
        )
        .collect();
    let mut reachable = BTreeSet::new();
    while let Some(name) = pending.pop() {
        if !reachable.insert(name) {
            continue;
        }
        if let Some(ty) = types.get(name) {
            // A `@computed` field is resolved on the way out, never decoded.
            pending.extend(
                ty.fields
                    .iter()
                    .filter(|field| !is_computed_field(field))
                    .map(|field| field.ty.name.as_str()),
            );
        }
    }
    reachable
}

fn first_validator(field: &Field) -> Option<&cratestack_core::Attribute> {
    field.attributes.iter().find(|attribute| {
        field_attribute_name(&attribute.raw).is_some_and(|name| VALIDATORS.contains(&name))
    })
}

/// One error per `type` that is not reachable from client input and has a
/// validator on a stored field, naming the first.
pub(super) fn validate_type_validator_reach(schema: &Schema, errors: &mut Vec<SchemaError>) {
    let reachable = client_reachable(schema);
    for ty in &schema.types {
        if reachable.contains(ty.name.as_str()) {
            continue;
        }
        let Some((field, attribute)) = ty
            .fields
            .iter()
            .filter(|field| !is_computed_field(field))
            .find_map(|field| first_validator(field).map(|attribute| (field, attribute)))
        else {
            continue;
        };
        errors.push(span_error(
            format!(
                "field `{}.{}` declares `{}`, but no client input reaches `{}`: validators run \
                 only on procedure arguments and `@computed` params, and `{}` is neither an \
                 argument type, the params type of a model's `@computed` field, nor held by \
                 one, so a client never sends a value of it and this validator would never run. \
                 It is refused. Take `{}` (or a `type` that holds it) as a procedure argument, \
                 or remove the validator",
                ty.name, field.name, attribute.raw, ty.name, ty.name, ty.name,
            ),
            attribute.span,
        ));
    }
}
