use crate::diagnostics::{SchemaError, span_error};

/// Parse and validate the `@isolation("...")` procedure attribute. At most
/// one is permitted per procedure; the level string must be one of the
/// values [`cratestack_core::TransactionIsolation::parse`] accepts.
pub(super) fn validate_procedure_isolation_attribute(
    procedure: &cratestack_core::Procedure,
) -> Result<(), SchemaError> {
    let matches: Vec<&cratestack_core::Attribute> = procedure
        .attributes
        .iter()
        .filter(|a| a.raw.starts_with("@isolation"))
        .collect();
    if matches.is_empty() {
        return Ok(());
    }
    if matches.len() > 1 {
        return Err(span_error(
            format!(
                "procedure `{}` declares more than one @isolation attribute",
                procedure.name,
            ),
            matches[1].span,
        ));
    }
    let attr = matches[0];
    let inner = attr
        .raw
        .strip_prefix("@isolation(")
        .and_then(|s| s.strip_suffix(')'))
        .ok_or_else(|| {
            span_error(
                format!(
                    "procedure `{}` @isolation requires a quoted level argument like @isolation(\"serializable\")",
                    procedure.name,
                ),
                attr.span,
            )
        })?
        .trim();
    let level = inner
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .ok_or_else(|| {
            span_error(
                format!(
                    "procedure `{}` @isolation argument must be a quoted string",
                    procedure.name,
                ),
                attr.span,
            )
        })?;
    cratestack_core::TransactionIsolation::parse(level).map_err(|error| {
        span_error(
            format!(
                "procedure `{}` @isolation: {}",
                procedure.name,
                error.public_message(),
            ),
            attr.span,
        )
    })?;
    Ok(())
}

/// Validate `@api_version("v1")` on procedures. The value is opaque to the
/// parser — banks pick their own scheme (semver, calver, mvX). We only
/// enforce non-empty and ASCII-printable so it can safely flow into URL
/// route segments.
pub(super) fn validate_procedure_api_version_attribute(
    procedure: &cratestack_core::Procedure,
) -> Result<(), SchemaError> {
    let matches: Vec<&cratestack_core::Attribute> = procedure
        .attributes
        .iter()
        .filter(|a| a.raw.starts_with("@api_version"))
        .collect();
    if matches.len() > 1 {
        return Err(span_error(
            format!(
                "procedure `{}` declares more than one @api_version attribute",
                procedure.name,
            ),
            matches[1].span,
        ));
    }
    let Some(attr) = matches.first() else {
        return Ok(());
    };
    let inner = attr
        .raw
        .strip_prefix("@api_version(")
        .and_then(|s| s.strip_suffix(')'))
        .ok_or_else(|| {
            span_error(
                format!(
                    "procedure `{}` @api_version requires a quoted version argument",
                    procedure.name,
                ),
                attr.span,
            )
        })?
        .trim();
    let stripped = inner
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .ok_or_else(|| {
            span_error(
                format!(
                    "procedure `{}` @api_version argument must be a quoted string",
                    procedure.name,
                ),
                attr.span,
            )
        })?;
    if stripped.is_empty() {
        return Err(span_error(
            format!(
                "procedure `{}` @api_version must not be empty",
                procedure.name,
            ),
            attr.span,
        ));
    }
    if !stripped
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
    {
        return Err(span_error(
            format!(
                "procedure `{}` @api_version must contain only alphanumeric, '.', '-', or '_' characters",
                procedure.name,
            ),
            attr.span,
        ));
    }
    Ok(())
}

/// Validate `@deprecated("use foo v2")` on procedures. Message is optional;
/// when present, the macro emits a `Deprecation: true` and `X-Deprecation`
/// header carrying the rationale.
pub(super) fn validate_procedure_deprecated_attribute(
    procedure: &cratestack_core::Procedure,
) -> Result<(), SchemaError> {
    let matches: Vec<&cratestack_core::Attribute> = procedure
        .attributes
        .iter()
        .filter(|a| a.raw == "@deprecated" || a.raw.starts_with("@deprecated("))
        .collect();
    if matches.len() > 1 {
        return Err(span_error(
            format!(
                "procedure `{}` declares more than one @deprecated attribute",
                procedure.name,
            ),
            matches[1].span,
        ));
    }
    let Some(attr) = matches.first() else {
        return Ok(());
    };
    if attr.raw == "@deprecated" {
        return Ok(());
    }
    let inner = attr
        .raw
        .strip_prefix("@deprecated(")
        .and_then(|s| s.strip_suffix(')'))
        .ok_or_else(|| {
            span_error(
                format!(
                    "procedure `{}` @deprecated must be either bare or `@deprecated(\"message\")`",
                    procedure.name,
                ),
                attr.span,
            )
        })?
        .trim();
    if !inner.starts_with('"') || !inner.ends_with('"') {
        return Err(span_error(
            format!(
                "procedure `{}` @deprecated argument must be a quoted string",
                procedure.name,
            ),
            attr.span,
        ));
    }
    Ok(())
}
