//! `@computed(params: <Type>?)` params-type resolution — split out of
//! [`super::computed`] to stay under the repo's ~200-LoC file convention.
//! Rule 4 in that module's doc comment: a params type must resolve to a
//! declared `type` block (not a model, builtin scalar, enum, or mixin)
//! and must not itself be computed-bearing.

use std::collections::BTreeSet;

use cratestack_core::{Field, Schema};

use crate::diagnostics::{SchemaError, span_error};
use crate::validate::type_names::BUILTIN_TYPES;

/// The declared-name sets a `@computed(params: <Type>?)` params type gets
/// checked against, bundled into one struct (rather than four positional
/// `BTreeSet` args) to keep [`validate_computed_params_type`] under
/// clippy's `too_many_arguments` threshold — the same reason
/// [`super::type_names::TypeRefAllow`] bundles its flags.
pub(super) struct ComputedParamsNameSets<'a> {
    type_decl_names: BTreeSet<&'a str>,
    model_names: &'a BTreeSet<&'a str>,
    enum_names: BTreeSet<&'a str>,
    mixin_names: BTreeSet<&'a str>,
}

impl<'a> ComputedParamsNameSets<'a> {
    pub(super) fn collect(schema: &'a Schema, model_names: &'a BTreeSet<&'a str>) -> Self {
        ComputedParamsNameSets {
            type_decl_names: schema.types.iter().map(|ty| ty.name.as_str()).collect(),
            model_names,
            enum_names: schema
                .enums
                .iter()
                .map(|enum_decl| enum_decl.name.as_str())
                .collect(),
            mixin_names: schema
                .mixins
                .iter()
                .map(|mixin| mixin.name.as_str())
                .collect(),
        }
    }
}

/// Validates the `<Type>` referenced by a `@computed(params: <Type>?)`
/// attribute: it must resolve to a declared `type` block that is not
/// itself computed-bearing. `field`/`owner_kind`/`owner_name` are only
/// used to build the error message; `params_type` is assumed to already
/// be a well-formed identifier (per-declaration validation runs first).
pub(super) fn validate_computed_params_type(
    owner_kind: &str,
    owner_name: &str,
    field: &Field,
    params_type: &str,
    name_sets: &ComputedParamsNameSets<'_>,
    bearing: &BTreeSet<String>,
) -> Result<(), SchemaError> {
    if name_sets.type_decl_names.contains(params_type) {
        if bearing.contains(params_type) {
            return Err(span_error(
                format!(
                    "field `{}` on {} `{}` uses `@computed(params: {}?)`, but `{}` itself \
                     contains `@computed` fields — computed params are decoded from the \
                     request, so a computed field inside the params type could never be \
                     resolved",
                    field.name, owner_kind, owner_name, params_type, params_type,
                ),
                field.span,
            ));
        }
        return Ok(());
    }

    let reason = if name_sets.model_names.contains(params_type) {
        format!("`{params_type}` is a model, not a declared `type` block")
    } else if name_sets.enum_names.contains(params_type) {
        format!("`{params_type}` is an enum, not a declared `type` block")
    } else if name_sets.mixin_names.contains(params_type) {
        format!("`{params_type}` is a mixin, not a declared `type` block")
    } else if BUILTIN_TYPES.contains(&params_type) {
        format!("`{params_type}` is a builtin scalar, not a declared `type` block")
    } else {
        format!("`{params_type}` is not declared anywhere in this schema")
    };

    Err(span_error(
        format!(
            "field `{}` on {} `{}` uses `@computed(params: {}?)`, but {} — computed params \
             must reference a declared `type` block (it has fields to decode a JSON payload \
             into; models, builtin scalars, enums, and mixins don't)",
            field.name, owner_kind, owner_name, params_type, reason,
        ),
        field.span,
    ))
}
