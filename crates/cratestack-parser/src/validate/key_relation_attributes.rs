//! Shape checks for the two field attributes cratestack#1074 found being
//! read loosely: `@id` and `@relation`.
//!
//! - `@id` takes no arguments. Only the bare spelling is the primary key
//!   ([`cratestack_core::is_primary_key_attribute`]); `@id(...)` is refused
//!   here instead of being left inert, because an inert `@id(...)` would
//!   quietly leave the field an ordinary column. Before #1074 every
//!   consumer but `cratestack-migrate` took any `@id…` prefix — `@identity`
//!   and `@idx` included — for the key; those names are now unknown
//!   attributes and go through `crate::validate::misspelled_attributes`
//!   like any other.
//! - A field declares at most one `@relation`. Every consumer (parser,
//!   macros, migrate, LSP, studio) reads the first and ignores the rest, so
//!   a second one was a contradiction that reported `schema OK`.
//!
//! Runs on every field-bearing declaration (`model`, `view`, `mixin`,
//! `type`, `auth`), after the removed/misspelled attribute checks.

use cratestack_core::{
    Field, field_attribute_name, is_primary_key_attribute, is_relation_attribute,
};

use crate::diagnostics::{SchemaError, span_error};

pub(super) fn validate_key_and_relation_attributes(
    owner_kind: &str,
    owner_name: &str,
    field: &Field,
) -> Result<(), SchemaError> {
    if let Some(attribute) = field.attributes.iter().find(|attribute| {
        field_attribute_name(&attribute.raw) == Some("id") && !is_primary_key_attribute(attribute)
    }) {
        return Err(span_error(
            format!(
                "field `{}` on {} `{}` writes `{}`, but `@id` takes no arguments — write `@id`",
                field.name, owner_kind, owner_name, attribute.raw,
            ),
            attribute.span,
        ));
    }

    if let Some(second) = field
        .attributes
        .iter()
        .filter(|attribute| is_relation_attribute(attribute))
        .nth(1)
    {
        return Err(span_error(
            format!(
                "field `{}` on {} `{}` declares `@relation` more than once; a field has at most \
                 one relation declaration, and the second (`{}`) would be ignored",
                field.name, owner_kind, owner_name, second.raw,
            ),
            second.span,
        ));
    }
    Ok(())
}
