use std::collections::BTreeSet;

use cratestack_core::{Field, TypeArity};

use crate::diagnostics::{SchemaError, span_error};

#[derive(Clone, Copy)]
pub(super) enum CustomFieldSupport {
    Rejected,
    TypeOnly,
}

pub(super) fn validate_custom_field_attribute(
    field: &Field,
    owner_kind: &str,
    owner_name: &str,
    support: CustomFieldSupport,
) -> Result<(), SchemaError> {
    let mut custom_count = 0usize;
    for attribute in &field.attributes {
        if !attribute.raw.starts_with("@custom") {
            continue;
        }
        custom_count += 1;
        if attribute.raw != "@custom" {
            return Err(span_error(
                format!(
                    "field `{}` on {} `{}` uses unsupported custom field directive `{}`; use bare `@custom` in this slice",
                    field.name, owner_kind, owner_name, attribute.raw,
                ),
                field.span,
            ));
        }
        if matches!(support, CustomFieldSupport::Rejected) {
            return Err(span_error(
                format!(
                    "field `{}` on {} `{}` cannot use `@custom`; resolver-backed custom fields are currently only supported on `type` declarations",
                    field.name, owner_kind, owner_name,
                ),
                field.span,
            ));
        }
    }

    if custom_count > 1 {
        return Err(span_error(
            format!(
                "field `{}` on {} `{}` declares `@custom` more than once",
                field.name, owner_kind, owner_name,
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
