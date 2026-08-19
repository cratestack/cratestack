//! Rejection of field attributes that the language used to accept and no
//! longer does.
//!
//! `.cstack` attributes parse generically into an opaque
//! `Attribute { raw, span }` (see `crate::parse::fields`), and there is no
//! blanket "unknown attribute" rejection pass — an unrecognised attribute is
//! simply inert. That default is fine for an attribute that never existed,
//! but it is the wrong answer for one that *did*: a schema carrying `@pb(3)`
//! from v0.8 would keep parsing after the v0.9 protobuf removal while
//! silently meaning nothing, and the author would get no signal that the
//! pins they wrote are now dead text.
//!
//! So removed attributes are rejected by name, individually, here. This is
//! deliberately not a generic unknown-attribute pass: adding one would
//! change the behaviour of every attribute the validators intentionally
//! ignore today, which is a far larger and unrelated language change.
//!
//! **When adding an entry to [`REMOVED_FIELD_ATTRIBUTES`], check every call
//! site.** Because this is opt-in per declaration kind rather than a single
//! central pass, a missed call site fails *silently* — the attribute goes
//! back to being inert, which is the exact bug this module exists to
//! prevent. There are five, one per field-bearing declaration: `model` and
//! `view` (`validate::models`, `validate::views`), and `mixin`, `type`, and
//! the `auth` block (all three in `validate::mixins_types`). Enum variants
//! are not one: `cratestack_core::EnumVariant` carries no attributes.
//! `tests_field_attrs::pb_field_attribute_is_rejected_on_every_field_bearing_declaration`
//! covers all five and is the guard against a sixth being added without a
//! matching call.

use cratestack_core::Field;

use crate::diagnostics::{SchemaError, span_error};

/// Attributes removed from the language, with the guidance shown when one is
/// still present in a schema.
///
/// Keyed by bare attribute name; matching also covers the `@name(...)`
/// argument form.
const REMOVED_FIELD_ATTRIBUTES: &[(&str, &str)] = &[(
    "@pb",
    "protobuf/gRPC support was removed in v0.9, so protobuf field numbers no \
     longer have any effect; delete the attribute (see docs/adr/0017-remove-grpc-protobuf.md)",
)];

pub(super) fn validate_removed_field_attributes(
    owner_kind: &str,
    owner_name: &str,
    field: &Field,
) -> Result<(), SchemaError> {
    for attribute in &field.attributes {
        for (name, guidance) in REMOVED_FIELD_ATTRIBUTES {
            if attribute.raw != *name && !attribute.raw.starts_with(&format!("{name}(")) {
                continue;
            }
            return Err(span_error(
                format!(
                    "field `{}` on {} `{}` uses `{}`, which is no longer supported: {}",
                    field.name, owner_kind, owner_name, name, guidance,
                ),
                field.span,
            ));
        }
    }
    Ok(())
}

// Tests live in `crate::tests_field_attrs` rather than here: they go
// through `parse_schema`, which exercises all three call sites (model,
// mixin, type) and the real user-facing message, instead of the helper in
// isolation.
