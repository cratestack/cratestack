//! The one check every field of every field-bearing declaration goes
//! through: `model`, `view`, `mixin`, `type` and the `auth` block (ADR 0019
//! D5). The lists are in `super::field_attribute_tables`.
//!
//! In order:
//!
//! 1. `super::removed_attributes`: `@allow`, `@deny`, `@pb` and `@custom`
//!    keep the specific explanation of what replaced them.
//! 2. `super::rename_attributes`: a field's `@rename` in the one form the
//!    migrator reads, where it is read, at most once. Its messages say why
//!    a view, a type or a relation field cannot rename.
//! 3. [`check_shape_hinted`] on each attribute, the check procedures,
//!    queries and `@@` attributes go through: the name must be in the
//!    kind's list, spelled exactly, with an argument list exactly when it
//!    takes one, nothing after it, and no second attribute run into it.
//!
//! The per-kind placement checks that give their own message for one name
//! (`@server_only` on a `type` or the `auth` block, `@computed` on a mixin,
//! a view or the `auth` block, `@id` on a mixin) run before this at each
//! call site, so their message wins; the tests in `tests_field_attribute_lists`
//! pin that those refusals and the lists agree.

use std::sync::OnceLock;

use cratestack_core::Field;

use super::attribute_shape::{Known, check_shape_hinted};
use super::field_attribute_tables::{
    AUTH_FIELD_ATTRIBUTES, MIXIN_FIELD_ATTRIBUTES, MODEL_FIELD_ATTRIBUTES, TYPE_FIELD_ATTRIBUTES,
    VIEW_FIELD_ATTRIBUTES,
};
use super::removed_attributes::{
    rejected_field_attribute_names, validate_removed_field_attributes,
};
use crate::diagnostics::SchemaError;

/// A declaration kind whose fields carry attributes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldHost {
    Model,
    View,
    Mixin,
    Type,
    Auth,
}

impl FieldHost {
    /// Every kind, in the order the language introduces them.
    pub const ALL: [FieldHost; 5] = [
        FieldHost::Model,
        FieldHost::View,
        FieldHost::Mixin,
        FieldHost::Type,
        FieldHost::Auth,
    ];

    /// How a diagnostic names the declaration (``field `x` on model `Y` ``).
    pub fn label(self) -> &'static str {
        match self {
            FieldHost::Model => "model",
            FieldHost::View => "view",
            FieldHost::Mixin => "mixin",
            FieldHost::Type => "type",
            FieldHost::Auth => "auth block",
        }
    }

    pub(super) fn table(self) -> &'static [Known] {
        match self {
            FieldHost::Model => MODEL_FIELD_ATTRIBUTES,
            FieldHost::View => VIEW_FIELD_ATTRIBUTES,
            FieldHost::Mixin => MIXIN_FIELD_ATTRIBUTES,
            FieldHost::Type => TYPE_FIELD_ATTRIBUTES,
            FieldHost::Auth => AUTH_FIELD_ATTRIBUTES,
        }
    }
}

/// The field attributes `host` accepts, sigil included (`@readonly`), in the
/// order its list is written. The validator and the editor completions read
/// the same list, so they cannot disagree.
pub fn field_attribute_names(host: FieldHost) -> Vec<&'static str> {
    host.table().iter().map(|(name, _)| *name).collect()
}

/// Every name a typo on a field might have meant: the names some kind
/// accepts, and the removed ones whose explanation a typo should reach.
pub(super) fn suggestion_pool() -> &'static [&'static str] {
    static POOL: OnceLock<Vec<&'static str>> = OnceLock::new();
    POOL.get_or_init(|| {
        let mut pool: Vec<&'static str> = FieldHost::ALL
            .iter()
            .flat_map(|host| field_attribute_names(*host))
            .chain(rejected_field_attribute_names())
            .collect();
        pool.sort_unstable();
        pool.dedup();
        pool
    })
}

/// Checks every attribute of `field`, a field of the `host` declaration
/// `owner_name`.
pub(super) fn validate_field_attributes(
    host: FieldHost,
    owner_name: &str,
    field: &Field,
) -> Result<(), SchemaError> {
    let kind = host.label();
    validate_removed_field_attributes(kind, owner_name, field)?;
    super::rename_attributes::validate_field_rename(kind, owner_name, field)?;
    let owner = format!("field `{}` on {kind} `{owner_name}`", field.name);
    let construct = format!("{kind} field");
    for attribute in &field.attributes {
        check_shape_hinted(
            attribute,
            host.table(),
            suggestion_pool(),
            &owner,
            &construct,
        )?;
    }
    Ok(())
}
