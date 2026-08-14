use cratestack_core::ExtensionKind;

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

/// Validate `@status(202)` on procedures. Declares the REST transport's
/// success-path (`Ok(...)`) HTTP status; `CratestackError`'s own status mapping
/// governs the `Err` branch unconditionally and is untouched by this
/// attribute (`crates/cratestack-axum/src/transport/encode_unary.rs`).
/// Restricted to `200..=299`: `CratestackError` already owns the 3xx/4xx/5xx
/// space, so anything outside 2xx here would create two competing sources
/// of truth for a response's error status. The exact 2xx boundary (e.g.
/// whether `200`/`204` should be rejected as redundant-or-nonsensical) is
/// an open design question the source issue explicitly reserves for the
/// maintainer — see cratestack#407 — so this only enforces "is a real
/// 2xx", nothing stricter.
///
/// Known limitation, deliberately left unenforced here (a maintainer
/// decision, not something to silently narrow): `@status(204)` is
/// accepted by this `200..=299` range check but `encode_response`
/// (`crates/cratestack-axum/src/transport/encode_unary.rs`) always
/// serializes and attaches a body regardless of status, so a declared
/// `204` currently produces a `204 No Content` response that carries a
/// body — a protocol violation per RFC 9110 §15.3.5. Tracked as a
/// follow-up rather than fixed here.
///
/// Transport-scoped: this attribute only has REST semantics
/// (`generate_procedure_axum_handler` threads it through the REST
/// success-path encoder). `generate_procedure_axum_handler` also backs
/// the RPC unary dispatch arm (`crate::transport::rpc`) — `#dispatch_ident`
/// is shared across REST and RPC — so an unrejected `@status` on a
/// `transport rpc` schema would silently become wire-visible there too
/// (`convert_handler_error_response` passes any `is_success()` HTTP
/// status through unchanged onto the RPC envelope). Rejected here at
/// schema-compile time instead of extended to RPC: whether RPC should
/// honour a declared status is a real design question left to the
/// maintainer, not something to decide by extending scope silently.
/// gRPC (`TransportStyle::Grpc`) is left unrestricted — tonic's gRPC
/// status model never reads the inner HTTP status this attribute
/// controls, so the combination is inert there, not silently wrong.
pub(super) fn validate_procedure_status_attribute(
    procedure: &cratestack_core::Procedure,
    schema: &cratestack_core::Schema,
) -> Result<(), SchemaError> {
    let matches: Vec<&cratestack_core::Attribute> = procedure
        .attributes
        .iter()
        .filter(|a| a.raw.starts_with("@status"))
        .collect();
    if matches.is_empty() {
        return Ok(());
    }
    if matches.len() > 1 {
        return Err(span_error(
            format!(
                "procedure `{}` declares more than one @status attribute",
                procedure.name,
            ),
            matches[1].span,
        ));
    }
    let attr = matches[0];
    if schema.transport == cratestack_core::TransportStyle::Rpc {
        return Err(span_error(
            format!(
                "procedure `{}` declares @status, which is a REST-only attribute, but this \
                 schema declares `transport rpc` — RPC unary dispatch shares the same handler \
                 REST uses, so @status would silently change the RPC response's HTTP status; \
                 remove @status from this procedure or switch the schema back to `transport rest`",
                procedure.name,
            ),
            attr.span,
        ));
    }
    let inner = attr
        .raw
        .strip_prefix("@status(")
        .and_then(|s| s.strip_suffix(')'))
        .ok_or_else(|| {
            span_error(
                format!(
                    "procedure `{}` @status requires a numeric status code argument like @status(202)",
                    procedure.name,
                ),
                attr.span,
            )
        })?
        .trim();
    let code: u16 = inner.parse().map_err(|_| {
        span_error(
            format!(
                "procedure `{}` @status argument must be an integer HTTP status code, got `{inner}`",
                procedure.name,
            ),
            attr.span,
        )
    })?;
    if !(200..=299).contains(&code) {
        return Err(span_error(
            format!(
                "procedure `{}` @status({code}) is outside the allowed 2xx range 200..=299 \
                 — non-2xx status is CratestackError's error-mapping's job, not @status's",
                procedure.name,
            ),
            attr.span,
        ));
    }
    Ok(())
}

/// Validate the bare `@no_rate_limit` procedure attribute
/// (`docs/design/extensions.md` §5). It takes no arguments (mirrors
/// `@deprecated`'s bare form above) and is only valid syntax when the
/// enclosing schema has declared `extension rate_limit { }` — declaring the
/// extension is what unlocks the attribute at all (layer 1 of the extension
/// model, `docs/design/extensions.md` §2); using it without that declaration
/// is a validation error here, distinct from and earlier than the Cargo
/// feature check `cratestack-macros`' `extension_gate` module performs at
/// macro-expansion time (layer 2).
pub(super) fn validate_procedure_no_rate_limit_attribute(
    procedure: &cratestack_core::Procedure,
    schema: &cratestack_core::Schema,
) -> Result<(), SchemaError> {
    let matches: Vec<&cratestack_core::Attribute> = procedure
        .attributes
        .iter()
        .filter(|a| a.raw == "@no_rate_limit" || a.raw.starts_with("@no_rate_limit("))
        .collect();
    if matches.is_empty() {
        return Ok(());
    }
    if matches.len() > 1 {
        return Err(span_error(
            format!(
                "procedure `{}` declares more than one @no_rate_limit attribute",
                procedure.name,
            ),
            matches[1].span,
        ));
    }
    let attr = matches[0];
    if attr.raw != "@no_rate_limit" {
        return Err(span_error(
            format!(
                "procedure `{}` @no_rate_limit does not take any arguments",
                procedure.name,
            ),
            attr.span,
        ));
    }
    if !schema
        .declared_extensions
        .contains(&ExtensionKind::RateLimit)
    {
        return Err(span_error(
            format!(
                "procedure `{}` uses @no_rate_limit, but this schema does not declare \
                 `extension rate_limit {{ }}` — add the extension block before opting a \
                 procedure out of rate limiting",
                procedure.name,
            ),
            attr.span,
        ));
    }
    Ok(())
}
