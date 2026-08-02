//! `collect_allowed_sort_keys` — the model's own top-level sortable
//! field names, consumed by the descriptor's `allowed_sorts` allow-list.
//!
//! Relation-nested sort keys (`"author.profile.nickname"`) are no longer
//! part of this list (cratestack#256): validating and dispatching those
//! is now the runtime resolver's job (`order_catalog.rs` +
//! `cratestack_sql::resolve_order_target`), not a pre-enumerated string
//! match. This function only ever walked one level deep for its own
//! purpose, so it stays cheap on its own — but it used to delegate to
//! the same recursive, path-enumerating walk that built the REST
//! match arms, which made even this flat list exponential to build.

use cratestack_core::Model;

use crate::shared::{model_name_set, scalar_model_fields};

pub(crate) fn collect_allowed_sort_keys(
    model: &Model,
    models: &[Model],
) -> Result<Vec<String>, String> {
    let model_names = model_name_set(models);
    Ok(scalar_model_fields(model, &model_names)
        .into_iter()
        .map(|field| field.name.clone())
        .collect())
}
