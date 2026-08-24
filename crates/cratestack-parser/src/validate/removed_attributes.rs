//! Rejection, by name, of field attributes that must never be inert:
//! attributes the language used to accept and no longer does, and
//! attributes that read as access control but were never wired up at field
//! position in the first place.
//!
//! `.cstack` attributes parse generically into an opaque
//! `Attribute { raw, span }` (see `crate::parse::fields`), and there is no
//! blanket "unknown attribute" rejection pass — an unrecognised attribute is
//! simply inert. That default is fine for an attribute that never existed,
//! but it is the wrong answer here:
//!
//! - a schema carrying `@pb(3)` from before 0.8.5 would keep parsing after
//!   the 0.8.5 protobuf removal while silently meaning nothing, and the
//!   author would get no signal that the pins they wrote are now dead text.
//! - `@allow(...)` / `@deny(...)` at field position parse and look exactly
//!   like the real, supported policy attributes of the same name at
//!   *procedure* position (`cratestack-macros/src/policy/procedure.rs`) and
//!   *model/view* position as `@@allow`/`@@deny` (double-`@`,
//!   `cratestack-macros/src/policy/model.rs`) — but no codegen reads a
//!   single-`@` `@allow`/`@deny` off a *field*. A schema author reading
//!   `bucket String @allow(auth().role == "system")` gets `schema OK` and
//!   reasonably believes the field is access-controlled; it is not
//!   (cratestack#679). Field-level read policy isn't implemented, so the
//!   only honest outcome is a loud parse error naming the real
//!   alternatives, not silent acceptance.
//!
//! So both classes are rejected by name, individually, here. This is
//! deliberately not a generic unknown-attribute pass: adding one would
//! change the behaviour of every attribute the validators intentionally
//! ignore today, which is a far larger and unrelated language change (see
//! cratestack#679's discussion — that issue's typo-class half, e.g.
//! `@raedonly` silently dropping `@readonly`, is intentionally NOT addressed
//! by this module).
//!
//! **When adding an entry to [`REJECTED_FIELD_ATTRIBUTES`], check every call
//! site.** Because this is opt-in per declaration kind rather than a single
//! central pass, a missed call site fails *silently* — the attribute goes
//! back to being inert, which is the exact bug this module exists to
//! prevent. There are five, one per field-bearing declaration: `model` and
//! `view` (`validate::models`, `validate::views`), and `mixin`, `type`, and
//! the `auth` block (all three in `validate::mixins_types`). Enum variants
//! are not one: `cratestack_core::EnumVariant` carries no attributes.
//! `tests_field_attrs::pb_field_attribute_is_rejected_on_every_field_bearing_declaration`
//! and its `@allow`/`@deny` counterparts cover all five and are the guard
//! against a sixth being added without a matching call.

use cratestack_core::Field;

use crate::diagnostics::{SchemaError, span_error};

/// Attributes rejected at field position, with the guidance shown when one
/// is still present in a schema.
///
/// Keyed by bare attribute name; matching also covers the `@name(...)`
/// argument form. Matching is on the bare single-`@` name exactly (or that
/// name followed by `(`), so it does not touch the double-`@` model/view
/// policy forms (`@@allow`, `@@deny`), which are unrelated, real, supported
/// attributes on a different declaration's attribute list.
const REJECTED_FIELD_ATTRIBUTES: &[(&str, &str)] = &[
    (
        "@custom",
        "`@custom` was replaced by `@computed` — the old attribute only ever generated an \
         inert resolver trait that nothing invoked; `@computed` (on `type` and `model` \
         fields) generates a resolver the framework actually calls when composing the \
         response. Rename the attribute to `@computed`",
    ),
    (
        "@pb",
        "protobuf/gRPC support was removed in 0.8.5, so protobuf field numbers no \
         longer have any effect; delete the attribute (see docs/adr/0017-remove-grpc-protobuf.md)",
    ),
    (
        "@allow",
        "field-level access policy is not supported and never was — it parses but no codegen \
         enforces it; use model-level `@@allow(\"read\", ...)` on the model/view for row \
         visibility, or `@readonly` / `@server_only` to keep the field out of inputs or out of \
         client responses",
    ),
    (
        "@deny",
        "field-level access policy is not supported and never was — it parses but no codegen \
         enforces it; use model-level `@@deny(\"read\", ...)` on the model/view for row \
         visibility, or `@readonly` / `@server_only` to keep the field out of inputs or out of \
         client responses",
    ),
];

pub(super) fn validate_removed_field_attributes(
    owner_kind: &str,
    owner_name: &str,
    field: &Field,
) -> Result<(), SchemaError> {
    for attribute in &field.attributes {
        for (name, guidance) in REJECTED_FIELD_ATTRIBUTES {
            if attribute.raw != *name && !attribute.raw.starts_with(&format!("{name}(")) {
                continue;
            }
            return Err(span_error(
                format!(
                    "field `{}` on {} `{}` uses `{}`, which is not supported at field \
                     position: {}",
                    field.name, owner_kind, owner_name, name, guidance,
                ),
                field.span,
            ));
        }
    }
    Ok(())
}

// Tests live in `crate::tests_field_attrs` rather than here: they go
// through `parse_schema`, which exercises all five call sites (model, view,
// mixin, type, auth block) and the real user-facing message, instead of the
// helper in isolation.
