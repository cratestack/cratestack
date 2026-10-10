//! Rename markers: a field's `@rename(from = "<old_column>")` and a
//! model's `@@rename(from = "<old_table>")` (GHSA-69g4-xvcm-vm2j).
//!
//! Their one reader is `cratestack migrate`
//! (`cratestack-migrate/src/convert/renames.rs`), which takes the *first*
//! attribute starting `@rename(` / `@@rename(` and reads it with
//! [`rename_marker_from`]; anything it cannot read counts as no marker,
//! and without a marker a renamed field or model is a drop plus an add —
//! the old column's data, or the old table's rows, gone. So:
//!
//! - **Field `@rename` takes exactly the form the migrator reads**, checked
//!   with that same reader: `@rename`, directly `(`, `from = "<old>"`, `)`.
//!   Before, only `@@rename` was checked, and `title String
//!   @rename(from: "name")` checked OK and dropped the `name` column. A
//!   misspelled name (`@renam(…)`, `@Rename(…)`) is refused by the field
//!   lists (`super::field_attributes`) with "did you mean `@rename`?";
//!   `@rename (…)` by the
//!   splitter (`crate::parse::attribute_spacing`). The check runs on every
//!   field-bearing block, so a mixin's marker is refused where it is
//!   written as well as in each model that `@use`s it.
//! - **A field's `@rename` is refused where the migrator never reads it**,
//!   in any form: the migrator projects only a model's stored columns, so
//!   a `@rename` on a field of a `view` (its columns come from its SQL,
//!   and a changed view is replaced, not altered), a `type` or the `auth`
//!   block (neither is a table), or on a relation field (`@relation(…)`,
//!   not a column) renames nothing — "accepted but inert", the class this
//!   advisory closes. A mixin's field is a model's once `@use`d, so its
//!   marker is read, unless it is a relation field. A `@computed` field's
//!   is already refused (`super::computed_attribute`).
//! - **A second marker is refused**, on a field or on a model, with the
//!   span on the second: the migrator reads only the first, so the second
//!   was silently ignored whichever name it carried. `@@rename`'s form is
//!   checked with the closed `@@` list (`super::block_attributes`), which
//!   runs first, so every `@@rename` counted here is readable.

use cratestack_core::schema::{RENAME_ARGUMENT_FORM, rename_marker_from};
use cratestack_core::{Attribute, Field, Model};

use crate::diagnostics::{SchemaError, span_error};

/// Whether `raw` is written with the attribute name `rename`, whatever
/// follows it.
fn is_field_rename(raw: &str) -> bool {
    raw.strip_prefix("@rename")
        .is_some_and(|rest| !rest.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_'))
}

fn refuse_second(owner: &str, marker: &str, first: &Attribute, second: &Attribute) -> SchemaError {
    span_error(
        format!(
            "{owner} declares a second `{marker}`, `{}`, after `{}`: `cratestack migrate` reads \
             only the first, so the second would be silently ignored. Declare one `{marker}`",
            second.raw, first.raw
        ),
        second.span,
    )
}

/// Why a `@rename` on `field` of an `owner_kind` block has no effect, or
/// `None` where the migrator reads it (module doc).
fn inert_rename(owner_kind: &str, field: &Field) -> Option<&'static str> {
    match owner_kind {
        "model" | "mixin" => field
            .attributes
            .iter()
            .any(|attribute| attribute.raw.starts_with("@relation("))
            .then_some("a relation field is not a column"),
        "view" => {
            Some("a view's columns come from its SQL, and a changed view is replaced, not altered")
        }
        _ => Some("the block is not a table"),
    }
}

/// A field's `@rename`: only on a model's stored column, in the one form
/// the migrator reads, at most once.
pub(super) fn validate_field_rename(
    owner_kind: &str,
    owner_name: &str,
    field: &Field,
) -> Result<(), SchemaError> {
    let owner = format!("field `{}` on {owner_kind} `{owner_name}`", field.name);
    let mut first: Option<&Attribute> = None;
    for attribute in &field.attributes {
        if !is_field_rename(&attribute.raw) {
            continue;
        }
        if let Some(why) = inert_rename(owner_kind, field) {
            return Err(span_error(
                format!(
                    "{owner} writes `{}`, which has no effect here: `cratestack migrate` reads \
                     `@rename` only on a stored column of a model, and {why}. It is refused; \
                     remove it",
                    attribute.raw
                ),
                attribute.span,
            ));
        }
        if rename_marker_from(&attribute.raw, "@rename").is_none() {
            return Err(span_error(
                format!(
                    "{owner} writes `{}`: `@rename` takes exactly one argument, \
                     `@rename({RENAME_ARGUMENT_FORM})` — the SQL column name being renamed, \
                     for example `@rename(from = \"name\")`, written directly after the name \
                     and ending the attribute. `cratestack migrate` reads no other form, and an \
                     unread marker makes the next migration drop the old column and add a new \
                     one, losing its data. It is refused",
                    attribute.raw
                ),
                attribute.span,
            ));
        }
        if let Some(first) = first {
            return Err(refuse_second(&owner, "@rename", first, attribute));
        }
        first = Some(attribute);
    }
    Ok(())
}

/// A model's `@@rename`, at most once. Runs after the closed `@@` list
/// has checked each one's form.
pub(super) fn validate_model_rename_count(model: &Model) -> Result<(), SchemaError> {
    let owner = format!("model `{}`", model.name);
    let mut renames = model
        .attributes
        .iter()
        .filter(|attribute| attribute.raw.starts_with("@@rename("));
    match (renames.next(), renames.next()) {
        (Some(first), Some(second)) => Err(refuse_second(&owner, "@@rename", first, second)),
        _ => Ok(()),
    }
}
