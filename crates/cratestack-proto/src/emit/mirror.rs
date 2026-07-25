//! Field-selection semantics mirrored from
//! `cratestack-client-typescript::{types, context}` (read in full for
//! ticket #169 — `visible_model_fields`, `scalar_model_fields`,
//! `model_allows_create`, `is_primary_key`, `is_generated_on_create`).
//! Reimplemented locally rather than depended on: this crate must not gain
//! a dependency on a client generator, the same "small pure helpers get
//! reimplemented per crate" convention `crate::casing` already follows for
//! the PascalCase/SCREAMING_SNAKE_CASE transforms.

use std::collections::BTreeSet;

use cratestack_core::{Field, Model, TypeDecl};

pub(crate) fn model_name_set(models: &[Model]) -> BTreeSet<&str> {
    models.iter().map(|model| model.name.as_str()).collect()
}

/// Fields visible on the generated `Model` message: everything except
/// `@server_only`. Relation fields stay in — the model message (unlike
/// Create/Update inputs) does project relations, as a reference to the
/// related message.
pub(crate) fn visible_model_fields(model: &Model) -> Vec<&Field> {
    model
        .fields
        .iter()
        .filter(|field| !is_server_only_field(field))
        .collect()
}

/// Fields visible on the generated `TypeDecl` message: everything except
/// `@server_only`. No create/update gating, no PK/default filtering — a
/// `type` isn't a model.
pub(crate) fn visible_type_fields(ty: &TypeDecl) -> Vec<&Field> {
    ty.fields
        .iter()
        .filter(|field| !is_server_only_field(field))
        .collect()
}

/// Base field set for `Create<M>Input`/`Update<M>Input`: relations and
/// `@server_only` fields excluded.
pub(crate) fn scalar_model_fields<'a>(
    model: &'a Model,
    model_names: &BTreeSet<&str>,
) -> Vec<&'a Field> {
    model
        .fields
        .iter()
        .filter(|field| !is_relation_field(model_names, field) && !is_server_only_field(field))
        .collect()
}

fn is_relation_field(model_names: &BTreeSet<&str>, field: &Field) -> bool {
    model_names.contains(field.ty.name.as_str())
}

fn is_server_only_field(field: &Field) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| attribute.raw == "@server_only")
}

/// Model has at least one `@@allow("create", ...)` or `@@allow("all", ...)`
/// rule. Mirrors the create verb's policy gate: a model without one
/// fail-closes on the server, so no `Create<M>Input` message is emitted.
pub(crate) fn model_allows_create(model: &Model) -> bool {
    model
        .attributes
        .iter()
        .filter_map(|attribute| allow_action(&attribute.raw))
        .any(|action| action == "create" || action == "all")
}

fn allow_action(raw: &str) -> Option<&str> {
    let inner = raw.trim().strip_prefix("@@allow(")?;
    let quote = inner.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let rest = &inner[quote.len_utf8()..];
    let end = rest.find(quote)?;
    Some(&rest[..end])
}

pub(crate) fn is_primary_key(field: &Field) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| attribute.raw.starts_with("@id"))
}

/// The model's single scalar `@id` field, if it has one. `None` for a
/// primary-key-less model — mirrors `cratestack-macros::transport::rpc`'s
/// own `model.fields.iter().find(|field| is_primary_key(field))`
/// (`generate_model_rpc_dispatch_arms`'s `pk_field`), which is what
/// `emit::service` and the grpc-only request-message synthesis key off of
/// to decide whether a model gets any CRUD service methods at all.
pub(crate) fn model_primary_key_field(model: &Model) -> Option<&Field> {
    model.fields.iter().find(|field| is_primary_key(field))
}

fn has_default(field: &Field) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| attribute.raw.starts_with("@default"))
}

pub(crate) fn is_generated_on_create(field: &Field) -> bool {
    has_default(field)
}
