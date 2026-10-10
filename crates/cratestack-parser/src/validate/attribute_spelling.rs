//! Block attributes written in a form the generators do not read.
//!
//! Generators recognise most attributes by their raw text, and many compare
//! it whole (`a.raw == "@@audit"`). So a no-argument block attribute with an
//! argument list it does not take, or two attributes with no space between
//! them, used to report `schema OK` and do nothing. Two rules, on the
//! block-level `@@` attributes of models and views:
//!
//! 1. **Run-on attributes.** A block-level attribute is its whole line, so
//!    `@@audit@@paged` / `@@audit @@paged` is one attribute whose second half
//!    nothing reads. See [`scan`]. A trailing `//` comment is never part of
//!    an attribute: the parser strips it first.
//! 2. **A no-argument attribute followed by anything but the end of its
//!    text**: an argument list (`@@audit()`) or stray punctuation. A
//!    following letter, digit or `_` makes another name, which is left to
//!    the closed list (`super::block_attributes`).
//!
//! The same two rules for a *field* attribute are part of the field's closed
//! list, `super::field_attributes`, checked by the same
//! `super::attribute_shape::check_shape` as procedures and queries; this
//! module used to hold a second copy of them for fields.
//!
//! # Block-level attributes that take no arguments
//!
//! | Attribute        | Readers | Other spellings, before |
//! |------------------|---------|-------------------------|
//! | `@@materialized` | `cratestack-core/src/schema/view.rs:73` via `has_bare_attribute` (`:114`): the name or `name(`… | arguments ignored; other punctuation no effect |
//! | `@@no_unique`    | `cratestack-core/src/schema/view.rs:79`, same | same |
//! | `@@audit`        | exact: `cratestack-macros/src/model/descriptor.rs:83` | arguments were already refused; other punctuation no effect |
//! | `@@soft_delete`  | exact: `cratestack-macros/src/model/descriptor.rs:87` | same |
//! | `@@subscribe`    | exact: `cratestack-macros/src/transport/subscribe_dispatch.rs:28`, `…/op_descriptors.rs:101` | same |
//!
//! `@@paged` is not listed: `super::model_attributes` already refuses every
//! spelling of it but the exact one.

mod block;
pub(super) mod scan;

pub(super) use block::validate_block_attribute_spelling;

/// The module doc's table, per block kind.
pub(super) const NO_ARGUMENT_VIEW_ATTRIBUTES: &[&str] = &["@@materialized", "@@no_unique"];
pub(super) const NO_ARGUMENT_MODEL_ATTRIBUTES: &[&str] =
    &["@@audit", "@@soft_delete", "@@subscribe"];

/// The text after `name` in `raw`, when `raw` is `name` followed by
/// something that does not continue the identifier.
fn trailing_text<'a>(raw: &'a str, name: &str) -> Option<&'a str> {
    raw.strip_prefix(name)
        .filter(|rest| !rest.is_empty())
        .filter(|rest| !rest.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_'))
}
