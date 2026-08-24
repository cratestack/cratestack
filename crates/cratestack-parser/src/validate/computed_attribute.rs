//! Per-declaration `@computed` field-attribute validation — bare form or
//! `@computed(params: <Type>?)`, at most once, only on the declaration
//! kinds that actually generate resolvers, and never combined with any
//! other field attribute. Split out of [`super::fields`] to stay under
//! the repo's ~200-LoC file convention.
//!
//! The schema-*wide* `@computed` rules (nested-computed types, procedure
//! argument/`@stream` restrictions, params-type resolution) live in
//! [`super::computed`]/[`super::computed_params`] and run after every
//! per-declaration validator here has already accepted the schema.

use cratestack_core::{
    Attribute, ComputedParamsArg, Field, is_computed_attribute, parse_computed_params_arg,
};

use crate::diagnostics::{SchemaError, span_error};

#[derive(Clone, Copy)]
pub(super) enum ComputedFieldSupport {
    Rejected,
    Supported,
}

/// Validate `@computed` at field position: bare form or `@computed(params:
/// <Type>?)`, at most once, only on the declarations that actually
/// generate resolvers (`type` and `model`), and never combined with any
/// other field attribute — a computed field is response-composition-only,
/// so every persistence / input / policy attribute (`@id`, `@default`,
/// `@unique`, `@readonly`, `@relation`, validators, ...) would be dead
/// text on it. Rejecting the whole class (rather than an allowlist of
/// known conflicts) fails closed for attributes added later.
pub(super) fn validate_computed_field_attribute(
    field: &Field,
    owner_kind: &str,
    owner_name: &str,
    support: ComputedFieldSupport,
) -> Result<(), SchemaError> {
    let mut computed_count = 0usize;
    for attribute in &field.attributes {
        if !is_computed_attribute(attribute) {
            continue;
        }
        computed_count += 1;
        if attribute.raw != "@computed" {
            validate_computed_argument_form(field, owner_kind, owner_name, attribute)?;
        }
        if matches!(support, ComputedFieldSupport::Rejected) {
            return Err(span_error(
                format!(
                    "field `{}` on {} `{}` cannot use `@computed`; resolver-backed computed fields are only supported on `type` and `model` declarations",
                    field.name, owner_kind, owner_name,
                ),
                field.span,
            ));
        }
    }

    if computed_count > 1 {
        return Err(span_error(
            format!(
                "field `{}` on {} `{}` declares `@computed` more than once",
                field.name, owner_kind, owner_name,
            ),
            field.span,
        ));
    }

    if computed_count == 1 && field.attributes.len() > 1 {
        let other = field
            .attributes
            .iter()
            .find(|attribute| !is_computed_attribute(attribute))
            .map(|attribute| attribute.raw.as_str())
            .unwrap_or_default();
        return Err(span_error(
            format!(
                "field `{}` on {} `{}` combines `@computed` with `{}`; a computed field is \
                 resolved at response-composition time and is never stored or accepted as \
                 input, so no other field attribute applies to it",
                field.name, owner_kind, owner_name, other,
            ),
            field.span,
        ));
    }

    Ok(())
}

/// Validates the parenthesized argument of a non-bare `@computed(...)`
/// attribute. The only accepted form is `@computed(params: <Type>?)`
/// (whitespace-tolerant around `:` and before the trailing `?`); the
/// trailing `?` is required because v1 always makes computed params
/// optional (see [`super::computed`] and `docs/design/computed-fields.md`).
/// Everything else — an unrelated argument, a missing colon, or the
/// well-formed-but-required `params: <Type>` spelling minus its `?` —
/// is an error.
fn validate_computed_argument_form(
    field: &Field,
    owner_kind: &str,
    owner_name: &str,
    attribute: &Attribute,
) -> Result<(), SchemaError> {
    let unsupported_directive_error = || {
        span_error(
            format!(
                "field `{}` on {} `{}` uses unsupported computed field directive `{}`; use bare `@computed` or `@computed(params: <Type>?)`",
                field.name, owner_kind, owner_name, attribute.raw,
            ),
            field.span,
        )
    };

    let Some(inner) = attribute
        .raw
        .strip_prefix("@computed(")
        .and_then(|rest| rest.strip_suffix(')'))
    else {
        return Err(unsupported_directive_error());
    };

    match parse_computed_params_arg(inner) {
        ComputedParamsArg::Optional(_) => Ok(()),
        ComputedParamsArg::Required(name) => Err(span_error(
            format!(
                "field `{}` on {} `{}` uses `@computed(params: {name})`; required computed \
                 params are not supported yet — add a trailing `?` (`@computed(params: \
                 {name}?)`). Params are always optional in v1: a required param would make \
                 plain CRUD reads unsatisfiable, and there is no wire slot for one on \
                 non-read paths.",
                field.name, owner_kind, owner_name,
            ),
            field.span,
        )),
        ComputedParamsArg::Unsupported => Err(unsupported_directive_error()),
    }
}
