//! `@pb(N)` field-attribute shape validation.
//!
//! No grammar change is needed here: `.cstack` attributes are already
//! parsed generically as an opaque `Attribute { raw, span }` (see
//! `crate::parse::fields`), so `@pb(123)` already parses today. This module
//! only adds the *shape* check — one `@pb` per field, a non-negative
//! integer argument, and not inside protobuf's own reserved field-number
//! range (`19000-19999`, rejected here rather than deferred to the lock
//! builder in `cratestack-proto`, since the range is invalid regardless of
//! what else is in the lock).
//!
//! `@pb(N)` is deliberately not validated on enum variants in this ticket:
//! [`cratestack_core::EnumVariant`] carries no `attributes: Vec<Attribute>`
//! field, so attaching `@pb(N)` to a variant would require a grammar
//! change, which is out of scope (the ticket's AC is "`@pb(N)` parses as a
//! field attribute", singular — fields only). Declared enum variants stay
//! auto-numbered only, with no pin override, until/unless a later ticket
//! adds variant-level attributes.

use cratestack_core::Field;

use crate::diagnostics::{SchemaError, span_error};

const PROTO_RESERVED_RANGE: std::ops::RangeInclusive<i64> = 19000..=19999;

pub(super) fn validate_pb_field_attribute(
    owner_kind: &str,
    owner_name: &str,
    field: &Field,
) -> Result<(), SchemaError> {
    let mut pb_count = 0usize;
    for attribute in &field.attributes {
        if attribute.raw != "@pb" && !attribute.raw.starts_with("@pb(") {
            continue;
        }
        pb_count += 1;
        if pb_count > 1 {
            return Err(span_error(
                format!(
                    "field `{}` on {} `{}` declares `@pb` more than once",
                    field.name, owner_kind, owner_name,
                ),
                field.span,
            ));
        }

        let number = parse_pb_number(&attribute.raw).map_err(|reason| {
            span_error(
                format!(
                    "field `{}` on {} `{}` has an invalid `@pb` attribute `{}`: {reason}",
                    field.name, owner_kind, owner_name, attribute.raw,
                ),
                field.span,
            )
        })?;

        if PROTO_RESERVED_RANGE.contains(&number) {
            return Err(span_error(
                format!(
                    "field `{}` on {} `{}` pins `@pb({number})`, which falls inside protobuf's \
                     own reserved field-number range ({}-{})",
                    field.name,
                    owner_kind,
                    owner_name,
                    PROTO_RESERVED_RANGE.start(),
                    PROTO_RESERVED_RANGE.end(),
                ),
                field.span,
            ));
        }
    }
    Ok(())
}

fn parse_pb_number(raw: &str) -> Result<i64, String> {
    let inner = raw
        .strip_prefix("@pb(")
        .and_then(|rest| rest.strip_suffix(')'))
        .ok_or_else(|| "expected `@pb(<non-negative integer>)`".to_owned())?;
    let trimmed = inner.trim();
    if trimmed.is_empty() {
        return Err("expected a non-negative integer argument, got none".to_owned());
    }
    trimmed
        .parse::<i64>()
        .ok()
        .filter(|n| *n >= 0)
        .ok_or_else(|| format!("expected a non-negative integer, got `{trimmed}`"))
}
