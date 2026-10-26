//! Rules R1–R5 of `docs/design/enum-ordinals.md` for an enum's explicit
//! ordinals: R1–R4 on the enum itself, R5 on the `auth` block.

use std::collections::BTreeMap;

use cratestack_core::{AuthBlock, ENUM_ORDINAL_MAX, ENUM_RESERVED_ATTRIBUTE, EnumDecl, Schema};

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
    // R1 above guarantees every variant is numbered.
    for (variant, ordinal) in e.variants.iter().filter_map(|v| Some((v, v.ordinal?))) {
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

/// R5: an `auth` field may not have an ordinal enum type. A claim is
/// whatever JSON the auth provider returns, so a policy could not know
/// whether `auth().role` holds the variant's name or its number, and
/// comparing an ordinal column with a string claim would fail open on the
/// create path. Ruling the type out of `auth` also rules out
/// `field == auth().x` for an ordinal field, since both sides must match.
pub(super) fn refuse_ordinal_auth_fields(
    schema: &Schema,
    auth: &AuthBlock,
) -> Result<(), SchemaError> {
    for field in &auth.fields {
        let ordinal = schema
            .enums
            .iter()
            .any(|e| e.name == field.ty.name && e.is_ordinal());
        if ordinal {
            return Err(span_error(
                format!(
                    "auth field `{}` has the ordinal enum type `{}`; a claim's encoding is the \
                     auth provider's choice, so `auth` fields may use string enums only. Use a \
                     string enum, or a `String`, here",
                    field.name, field.ty.name
                ),
                field.span,
            ));
        }
    }
    Ok(())
}
