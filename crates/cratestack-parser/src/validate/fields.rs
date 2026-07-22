use cratestack_core::Field;

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
