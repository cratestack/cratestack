//! Field/model attribute predicates this crate needs for the same
//! reason `crates/cratestack-macros/src/shared/attrs.rs` has them —
//! whether a field is the primary key, is excluded from the client
//! projection, or a model paginates its `list` route. Not imported from
//! `cratestack-macros` (its predicates are `pub(crate)` to that crate,
//! and pulling in `cratestack-macros` as a dependency here would be a
//! much heavier edge than three one-line string checks against
//! `Attribute::raw` warrant) — mirrored instead, deliberately kept this
//! small so drift from the real predicates stays easy to spot in review.

use std::collections::BTreeSet;

use cratestack_core::{Field, Model};

/// Field carries an `@id`-prefixed attribute — the model's primary key.
pub(crate) fn is_primary_key(field: &Field) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| attribute.raw.starts_with("@id"))
}

/// Field carries `@server_only` — never serialized to a client, so
/// excluded from a synthesized record the same way
/// `crates/cratestack-macros/src/axum/model/serializers/
/// projection_fields.rs`'s default projection excludes it.
pub(crate) fn is_server_only_field(field: &Field) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| attribute.raw == "@server_only")
}

/// Model carries `@@paged` — its `list` route returns the
/// `{items, totalCount, pageInfo}` envelope instead of a bare array.
pub(crate) fn is_paged_model(model: &Model) -> bool {
    model
        .attributes
        .iter()
        .any(|attribute| attribute.raw == "@@paged")
}

/// Field's type names another declared model — a relation field,
/// populated only via `include=<relation>` and excluded from the
/// default projection this generator synthesizes.
pub(crate) fn is_relation_field(model_names: &BTreeSet<&str>, field: &Field) -> bool {
    model_names.contains(field.ty.name.as_str())
}
