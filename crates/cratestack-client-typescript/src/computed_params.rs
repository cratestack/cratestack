//! `@computed`/`@computed(params: <Type>?)` field predicates
//! (`docs/design/computed-fields.md`), split out of `crate::types` per
//! the repo's 200-LoC file convention and re-exported there so every
//! existing `use crate::types::{...}` call site keeps working unchanged.

use cratestack_core::{Field, Model, computed_params_type_name};

/// Field carries `@computed`/`@computed(params: <Type>?)`
/// (`docs/design/computed-fields.md`) — resolved at response time, never
/// stored. Unlike a relation or `@server_only` field, a computed field
/// IS part of the default model projection (`scalar_model_fields`/
/// `visible_model_fields` deliberately do NOT exclude it — a computed
/// field is exactly as "scalar" as any other leaf field from the wire's
/// point of view), so call sites that need to exclude it (create/update
/// inputs, `Where`/`SortField` builders) check this explicitly.
pub(crate) fn is_computed_field(field: &Field) -> bool {
    cratestack_core::is_computed_field(field)
}

/// The model's own `@computed(params: <Type>?)` fields, paired with each
/// one's declared params type name — the source data for the generated
/// `<Model>ComputedParams` interface (`crate::views::build_computed_params_interface`).
/// A bare `@computed` field (no `params:` argument) never contributes here:
/// [`cratestack_core::computed_params_type_name`] returns `None` for it,
/// same as for a field with no `@computed` attribute at all.
pub(crate) fn computed_params_fields(model: &Model) -> Vec<(&Field, &str)> {
    model
        .fields
        .iter()
        .filter_map(|field| {
            computed_params_type_name(field).map(|params_type| (field, params_type))
        })
        .collect()
}

/// Whether `model` declares at least one *parameterized*
/// `@computed(params: <Type>?)` field — not merely `@computed` in general.
/// Gates the generated `get`/`list` `computedParams` surface per model: the
/// server 422s a `computedParams` key that doesn't name a parameterized
/// field, so a model whose only computed fields are bare (or with none at
/// all) must not accept the parameter in the first place — it could never
/// be satisfied. Mirrors `cratestack-client-dart`'s
/// `ModelApiView::has_parameterized_computed_fields` predicate exactly
/// (same server-side reasoning, different client).
pub(crate) fn has_parameterized_computed_fields(model: &Model) -> bool {
    !computed_params_fields(model).is_empty()
}
