//! Rules R1–R4 of `docs/design/enum-ordinals.md` for an enum's explicit
//! ordinals, and the refusal that keeps them from being declared but inert
//! until a generator stores and sends them.

use std::collections::BTreeMap;

use cratestack_core::{ENUM_ORDINAL_MAX, ENUM_RESERVED_ATTRIBUTE, EnumDecl};

use crate::diagnostics::{SchemaError, span_error};

/// R1 all or none, R2 range and uniqueness, R3 a reserved ordinal stays
/// unused, R4 `@@reserved` only on an ordinal enum.
pub(super) fn validate_enum_ordinals(e: &EnumDecl) -> Result<(), SchemaError> {
    let numbered = e.variants.iter().filter(|v| v.ordinal.is_some()).count();
    if numbered == 0 {
        if let Some(reserved) = &e.reserved {
            return Err(span_error(
                format!(
                    "`{ENUM_RESERVED_ATTRIBUTE}` retires ordinals, but no variant of enum `{}` \
                     has one; number every variant (`Name = 1`) or remove the line",
                    e.name
                ),
                reserved.span,
            ));
        }
        return Ok(());
    }
    if let Some(bare) = e.variants.iter().find(|v| v.ordinal.is_none()) {
        return Err(span_error(
            format!(
                "enum `{}` numbers some variants but not `{}`; give every variant an \
                 ordinal (`{} = n`) or none",
                e.name, bare.name, bare.name
            ),
            bare.span,
        ));
    }

    let mut used = BTreeMap::new();
    for variant in &e.variants {
        let ordinal = variant.ordinal.expect("every variant is numbered (R1)");
        if !(0..=ENUM_ORDINAL_MAX).contains(&ordinal) {
            return Err(span_error(
                format!(
                    "the ordinal of variant `{}` must be an integer from 0 to \
                     {ENUM_ORDINAL_MAX}; {ordinal} is not",
                    variant.name
                ),
                variant.span,
            ));
        }
        if let Some(first) = used.insert(ordinal, variant.name.as_str()) {
            return Err(span_error(
                format!(
                    "ordinal {ordinal} is used by both `{first}` and `{}` on enum `{}`; \
                     each variant needs its own",
                    variant.name, e.name
                ),
                variant.span,
            ));
        }
    }

    let Some(reserved) = &e.reserved else {
        return Ok(());
    };
    let mut retired = BTreeMap::new();
    for &value in &reserved.values {
        if !(0..=ENUM_ORDINAL_MAX).contains(&value) {
            return Err(span_error(
                format!(
                    "`{ENUM_RESERVED_ATTRIBUTE}` on enum `{}` lists {value}, outside the \
                     ordinal range 0 to {ENUM_ORDINAL_MAX}",
                    e.name
                ),
                reserved.span,
            ));
        }
        if retired.insert(value, ()).is_some() {
            return Err(span_error(
                format!(
                    "`{ENUM_RESERVED_ATTRIBUTE}` on enum `{}` lists {value} twice",
                    e.name
                ),
                reserved.span,
            ));
        }
        if let Some(name) = used.get(&value) {
            return Err(span_error(
                format!(
                    "variant `{name}` uses ordinal {value}, which `{ENUM_RESERVED_ATTRIBUTE}` \
                     retires on enum `{}`; a retired ordinal is never reused, so give \
                     `{name}` a new one",
                    e.name
                ),
                reserved.span,
            ));
        }
    }
    Ok(())
}

/// Nothing generates from an ordinal yet: every backend and client would
/// still store and send the variant's name. Accepting `Pending = 1` would
/// therefore promise an encoding the build does not deliver, so a
/// well-formed ordinal enum is refused until the generators land.
pub(super) fn refuse_until_generated(e: &EnumDecl) -> Result<(), SchemaError> {
    if e.variants.iter().all(|v| v.ordinal.is_none()) {
        return Ok(());
    }
    Err(span_error(
        format!(
            "enum `{}` declares explicit ordinals, which this release parses but does not \
             generate: every backend and client would still store and send the variant \
             names. Remove the `= n` values and any `{ENUM_RESERVED_ATTRIBUTE}` line until \
             ordinal enums ship (docs/design/enum-ordinals.md in the cratestack repository)",
            e.name
        ),
        e.name_span,
    ))
}
