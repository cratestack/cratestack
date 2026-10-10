//! Specific explanations for field attributes that must never be inert:
//! attributes the language used to accept and no longer does, and
//! attributes that read as access control but were never wired up at field
//! position in the first place.
//!
//! `.cstack` attributes parse generically into an opaque
//! `Attribute { raw, span }` (see `crate::parse::fields`). Since ADR 0019 D5
//! every field attribute outside its kind's list is refused by
//! `super::field_attributes` (`super::field_attribute_tables`), so none of
//! these could pass silently any more. They are still named here, and run
//! first, because the generic "unsupported attribute" would not say what to
//! do instead:
//!
//! - a schema carrying `@pb(3)` from before 0.8.5 has pins that are dead
//!   text since the 0.8.5 protobuf removal; the author needs to be told that
//!   is why, not only that the name is unknown.
//! - `@allow(...)` / `@deny(...)` at field position parse and look exactly
//!   like the real, supported policy attributes of the same name at
//!   *procedure* position (`cratestack-macros/src/policy/procedure.rs`) and
//!   *model/view* position as `@@allow`/`@@deny` (double-`@`,
//!   `cratestack-macros/src/policy/model.rs`) — but no codegen reads a
//!   single-`@` `@allow`/`@deny` off a *field*. A schema author reading
//!   `bucket String @allow(auth().role == "system")` reasonably believes the
//!   field is access-controlled; it is not (cratestack#679). Field-level read
//!   policy isn't implemented, so the message names the real alternatives.
//!
//! The names are also offered as suggestions for a typo, so `@alow` is
//! pointed at `@allow` and then gets this explanation.
//!
//! **When adding an entry to [`REJECTED_FIELD_ATTRIBUTES`], check every call
//! site.** There is one, `super::field_attributes::validate_field_attributes`,
//! which every field-bearing declaration goes through: `model` and `view`
//! (`validate::models`, `validate::views`), and `mixin`, `type`, and the
//! `auth` block (all three in `validate::mixins_types`). Enum variants are
//! not one: `cratestack_core::EnumVariant` carries no attributes.
//! `tests_field_attrs::pb_field_attribute_is_rejected_on_every_field_bearing_declaration`
//! and its `@allow`/`@deny` counterparts cover all five.

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
         visibility, or `@readonly` / `@server_only` on a model field to keep it out of \
         inputs or out of client responses",
    ),
    (
        "@deny",
        "field-level access policy is not supported and never was — it parses but no codegen \
         enforces it; use model-level `@@deny(\"read\", ...)` on the model/view for row \
         visibility, or `@readonly` / `@server_only` on a model field to keep it out of \
         inputs or out of client responses",
    ),
];

/// The removed names, sigil included (`@pb`).
pub(super) fn rejected_field_attribute_names() -> impl Iterator<Item = &'static str> {
    REJECTED_FIELD_ATTRIBUTES.iter().map(|(name, _)| *name)
}

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
