use std::collections::BTreeSet;

use cratestack_core::{Field, TypeArity};

use crate::diagnostics::{SchemaError, span_error};
use crate::validate::reserved_idents::validate_reserved_identifier;

#[derive(Clone, Copy)]
pub(super) enum ComputedFieldSupport {
    Rejected,
    Supported,
}

/// Validate `@computed` at field position: bare form only, at most once,
/// only on the declarations that actually generate resolvers (`type` and
/// `model`), and never combined with any other field attribute — a
/// computed field is response-composition-only, so every persistence /
/// input / policy attribute (`@id`, `@default`, `@unique`, `@readonly`,
/// `@relation`, validators, ...) would be dead text on it. Rejecting the
/// whole class (rather than an allowlist of known conflicts) fails closed
/// for attributes added later.
pub(super) fn validate_computed_field_attribute(
    field: &Field,
    owner_kind: &str,
    owner_name: &str,
    support: ComputedFieldSupport,
) -> Result<(), SchemaError> {
    let mut computed_count = 0usize;
    for attribute in &field.attributes {
        if !attribute.raw.starts_with("@computed") {
            continue;
        }
        computed_count += 1;
        if attribute.raw != "@computed" {
            return Err(span_error(
                format!(
                    "field `{}` on {} `{}` uses unsupported computed field directive `{}`; use bare `@computed`",
                    field.name, owner_kind, owner_name, attribute.raw,
                ),
                field.span,
            ));
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
            .find(|attribute| attribute.raw != "@computed")
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

/// Reject `@readonly` / `@server_only` declared on the primary-key field —
/// PKs are server-controlled anyway and the combination is a likely typo.
pub(super) fn validate_field_policy_attributes(
    model_name: &str,
    field: &cratestack_core::Field,
) -> Result<(), SchemaError> {
    let is_id = field.attributes.iter().any(|a| a.raw.starts_with("@id"));
    let has_readonly = field.attributes.iter().any(|a| a.raw == "@readonly");
    let has_server_only = field.attributes.iter().any(|a| a.raw == "@server_only");

    if is_id && (has_readonly || has_server_only) {
        let attr = if has_readonly {
            "@readonly"
        } else {
            "@server_only"
        };
        return Err(span_error(
            format!(
                "field `{}.{}` is the primary key and must not declare {attr}",
                model_name, field.name,
            ),
            field.span,
        ));
    }
    if has_readonly && has_server_only {
        return Err(span_error(
            format!(
                "field `{}.{}` declares both @readonly and @server_only; use @server_only alone",
                model_name, field.name,
            ),
            field.span,
        ));
    }
    Ok(())
}

/// Reject `@default(dbgenerated(...))` with an argument. cratestack's
/// `dbgenerated()` is a bare marker (matching Prisma's semantics): it
/// asserts the column already has a real Postgres-level default set
/// some other way (hand-authored migration SQL, a trigger,
/// `GENERATED ... AS IDENTITY`, etc), and the migration emitter never
/// generates a `DEFAULT` clause for it. An argument would silently be
/// discarded rather than turned into real SQL, which is worse than
/// rejecting it outright.
pub(super) fn validate_default_dbgenerated_no_args(
    model_name: &str,
    field: &cratestack_core::Field,
) -> Result<(), SchemaError> {
    let Some(attribute) = field
        .attributes
        .iter()
        .find(|attribute| attribute.raw.starts_with("@default("))
    else {
        return Ok(());
    };
    let Some(inner) = attribute
        .raw
        .strip_prefix("@default(")
        .and_then(|rest| rest.strip_suffix(')'))
    else {
        return Ok(());
    };
    let Some(args) = inner
        .trim()
        .strip_prefix("dbgenerated(")
        .and_then(|rest| rest.strip_suffix(')'))
    else {
        return Ok(());
    };
    if !args.trim().is_empty() {
        return Err(span_error(
            format!(
                "field `{}.{}` uses `@default(dbgenerated({}))`; cratestack's \
                 `dbgenerated()` takes no argument — it is a marker meaning the column \
                 already has a real Postgres-level default set some other way \
                 (hand-authored migration SQL, a trigger, `GENERATED ... AS IDENTITY`, \
                 etc). Remove the argument and use bare `dbgenerated()`.",
                model_name,
                field.name,
                args.trim(),
            ),
            field.span,
        ));
    }
    Ok(())
}

/// Reject a field named after a Rust keyword with no valid identifier
/// spelling at all — `self`, `Self`, `super`, `crate`. Every other Rust
/// keyword (`match`, `type`, `ref`, `move`, `impl`, `fn`, `let`, `loop`,
/// `box`, ...) is escaped as a raw identifier (`r#type`) at codegen time by
/// `cratestack_macros::shared::ident` and needs no rejection here — see
/// cratestack#398. These four are different: `r#self`/`r#Self`/`r#super`/
/// `r#crate` are not valid Rust at all (rustc rejects them outright), so
/// there is no escape hatch, and the field must be renamed.
///
/// Thin, field-shaped wrapper over the general
/// [`crate::validate::reserved_idents::validate_reserved_identifier`],
/// which also covers every other ident site the codegen `ident()` helper
/// touches (model/mixin/type/view/procedure names, enum names/variants,
/// procedure argument names).
pub(super) fn validate_field_reserved_identifier(
    field: &cratestack_core::Field,
    owner_kind: &str,
    owner_name: &str,
) -> Result<(), SchemaError> {
    validate_reserved_identifier(
        &field.name,
        field.span,
        &format!("field `{}` on {owner_kind} `{owner_name}`", field.name),
    )
}

/// Reject list-arity scalar/enum model fields on any schema that declares a
/// `datasource`. `TypeArity::List` is otherwise accepted by the parser and
/// turned into real `{base}[]` Postgres DDL by `cratestack-migrate`, but
/// `cratestack-macros`'s `sql_value_tokens` has no bind representation for
/// any list-valued scalar or enum — every such field panics at
/// `include_server_schema!` / `include_embedded_schema!` expansion instead
/// of failing here, at the field that actually causes the problem (see
/// cratestack#229).
///
/// Scoped to `datasource`-bearing schemas only: a schema with no
/// `datasource` can only be consumed through `include_client_schema!`,
/// which never binds SQL values, so list-valued scalar/enum fields are
/// genuinely fine there (see
/// `crates/cratestack-client-dart/tests/fixtures/enums.cstack`, whose
/// `model User { roles Role[] }` is exercised as a passing test today).
///
/// A list-arity field whose type name is another model is a to-many
/// `@relation` and is unaffected — those are validated separately in
/// [`super::models::validate_field_relation`] and have real codegen support
/// (`cratestack-macros/src/relation/`).
pub(super) fn validate_field_list_arity_support(
    schema_has_datasource: bool,
    model_name: &str,
    model_names: &BTreeSet<&str>,
    field: &Field,
) -> Result<(), SchemaError> {
    if !schema_has_datasource {
        return Ok(());
    }
    if field.ty.arity != TypeArity::List {
        return Ok(());
    }
    if model_names.contains(field.ty.name.as_str()) {
        return Ok(());
    }

    Err(span_error(
        format!(
            "model `{model_name}` field `{}`: list-valued type `{}[]` is not supported on a \
             database-backed model — there is no SQL bind representation for a list-valued \
             scalar or enum yet, so this schema would parse and emit valid DDL but panic at \
             `include_server_schema!`/`include_embedded_schema!` expansion. Use a single \
             `{}` value, model this as a `@relation` to another model, or drop the \
             `datasource` block if this schema is only ever consumed via \
             `include_client_schema!`.",
            field.name, field.ty.name, field.ty.name,
        ),
        field.span,
    ))
}
