//! Schema-wide semantic checks for `@computed` fields.
//!
//! The per-declaration rules (bare form, at-most-once, no other
//! attributes, which declaration kinds accept it at all) live in
//! [`super::fields::validate_computed_field_attribute`] and run inside
//! each declaration's own validator. This module holds the rules that
//! need the *whole* schema:
//!
//! 1. A computed field's own type must be a plain value — never a
//!    `model` (on a model that spelling is relation syntax; everywhere
//!    it would force the resolver to fabricate a row that never came
//!    from the database), and never a computed-bearing `type` (the
//!    framework resolves computed fields on values *it* walks; a
//!    resolver's return value is serialized as-is, so nested computed
//!    fields inside it would silently ship unresolved).
//! 2. Procedure *arguments* must not reference a computed-bearing type
//!    or model, directly or through nested `type` fields / generic
//!    arguments: the client-side shape of a computed owner includes the
//!    computed field, the server-side shape doesn't, so a
//!    computed-bearing input would silently drop data on decode.
//! 3. `@stream` procedures must not stream computed-bearing items —
//!    item-by-item resolution inside the incremental HTTP encoder is
//!    not implemented yet, and silently streaming unresolved values
//!    would be worse than rejecting.
//! 4. A `@computed(params: <Type>?)` params type must resolve to a
//!    declared `type` block — not a model (no CRUD input surface to
//!    validate a params payload against), not a builtin scalar/enum (no
//!    fields to decode a JSON object into), not a mixin (never a
//!    standalone type on the wire) — and must not itself be
//!    computed-bearing (params are decoded from the request, so a
//!    computed field inside one could never be resolved).

use std::collections::BTreeSet;

use cratestack_core::{Field, Schema, TypeRef, computed_params_type_name};

use crate::diagnostics::{SchemaError, span_error};
use crate::validate::computed_params::{ComputedParamsNameSets, validate_computed_params_type};

/// True for a field carrying either spelling of `@computed` — bare or
/// `@computed(params: <Type>?)`. By the time this module runs,
/// per-declaration validation has already rejected any other spelling
/// (see [`super::fields::validate_computed_field_attribute`]), so
/// `starts_with` is safe to use as the sole discriminator here.
fn is_computed(field: &Field) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| attribute.raw.starts_with("@computed"))
}

/// Names (of `type` declarations and `model`s) whose wire shape contains
/// at least one `@computed` field, directly or through nested `type`
/// fields. Model relation fields do not propagate: a relation is never
/// embedded in the model's own wire struct.
pub(super) fn computed_bearing_names(schema: &Schema) -> BTreeSet<String> {
    let mut bearing: BTreeSet<String> = schema
        .models
        .iter()
        .filter(|model| model.fields.iter().any(is_computed))
        .map(|model| model.name.clone())
        .chain(
            schema
                .types
                .iter()
                .filter(|ty| ty.fields.iter().any(is_computed))
                .map(|ty| ty.name.clone()),
        )
        .collect();

    loop {
        let mut grew = false;
        for ty in &schema.types {
            if bearing.contains(&ty.name) {
                continue;
            }
            if ty
                .fields
                .iter()
                .any(|field| bearing.contains(&field.ty.name))
            {
                bearing.insert(ty.name.clone());
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    bearing
}

/// Any type-declaration or model name referenced by `ty`, including
/// through generic arguments (`Page<T>`, `FindMany<T>`, ...), that is in
/// `bearing`.
fn first_bearing_reference<'a>(ty: &'a TypeRef, bearing: &BTreeSet<String>) -> Option<&'a str> {
    if bearing.contains(&ty.name) {
        return Some(&ty.name);
    }
    ty.generic_args
        .iter()
        .find_map(|arg| first_bearing_reference(arg, bearing))
}

pub(super) fn validate_computed(schema: &Schema) -> Result<(), SchemaError> {
    let model_names: BTreeSet<&str> = schema
        .models
        .iter()
        .map(|model| model.name.as_str())
        .collect();
    let name_sets = ComputedParamsNameSets::collect(schema, &model_names);
    let bearing = computed_bearing_names(schema);

    let owners = schema
        .models
        .iter()
        .map(|model| ("model", &model.name, &model.fields))
        .chain(schema.types.iter().map(|ty| ("type", &ty.name, &ty.fields)));
    for (owner_kind, owner_name, fields) in owners {
        for field in fields.iter().filter(|field| is_computed(field)) {
            if model_names.contains(field.ty.name.as_str()) {
                return Err(span_error(
                    format!(
                        "field `{}` on {} `{}` is `@computed` but its type `{}` is a model — \
                         a computed field's resolver must return a plain value (scalar, enum, \
                         or non-computed `type`), not a database row",
                        field.name, owner_kind, owner_name, field.ty.name,
                    ),
                    field.span,
                ));
            }
            if bearing.contains(&field.ty.name) {
                return Err(span_error(
                    format!(
                        "field `{}` on {} `{}` is `@computed` but its type `{}` itself \
                         contains `@computed` fields — resolver return values are serialized \
                         as-is, so nested computed fields inside them would never be resolved",
                        field.name, owner_kind, owner_name, field.ty.name,
                    ),
                    field.span,
                ));
            }
            if let Some(params_type) = computed_params_type_name(field) {
                validate_computed_params_type(
                    owner_kind,
                    owner_name,
                    field,
                    params_type,
                    &name_sets,
                    &bearing,
                )?;
            }
        }
    }

    for procedure in &schema.procedures {
        for arg in &procedure.args {
            if let Some(name) = first_bearing_reference(&arg.ty, &bearing) {
                return Err(span_error(
                    format!(
                        "procedure `{}` argument `{}` references `{}`, which contains \
                         `@computed` fields — computed fields exist only in responses (the \
                         client-side shape includes them, the server-side shape doesn't), so \
                         a computed-bearing type cannot be used as procedure input",
                        procedure.name, arg.name, name,
                    ),
                    arg.span,
                ));
            }
        }
        let is_stream = procedure.attributes.iter().any(|a| a.raw == "@stream");
        if is_stream && let Some(name) = first_bearing_reference(&procedure.return_type, &bearing) {
            return Err(span_error(
                format!(
                    "procedure `{}` declares @stream but returns `{}`, which contains \
                     `@computed` fields — computed-field resolution inside the incremental \
                     stream encoder is not supported yet; drop @stream (buffered list \
                     responses resolve computed fields) or remove the computed field",
                    procedure.name, name,
                ),
                procedure.span,
            ));
        }
    }

    Ok(())
}
