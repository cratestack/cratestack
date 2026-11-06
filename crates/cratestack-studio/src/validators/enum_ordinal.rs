//! Ordinal enum payload checks (`Pending = 1`,
//! `docs/design/enum-ordinals.md`).
//!
//! The column stores the number, and either form is accepted, as the
//! generated server accepts either: the variant's name (what the UI's
//! `<select>` sends) or the ordinal itself. The binders map a name to its
//! ordinal through the same [`ordinal_of`], so the two cannot disagree.

use cratestack_core::{EnumDecl, Field};

use super::types::{FieldError, ValidationCode};
use crate::data::common::ordinal_of;

/// `None` when `value` names a variant of `e` or is one of its ordinals.
pub(super) fn check(field: &Field, e: &EnumDecl, value: &serde_json::Value) -> Option<FieldError> {
    if ordinal_of(e, value).is_some() {
        return None;
    }
    let accepted = e
        .variants
        .iter()
        .filter_map(|v| Some(format!("{} ({})", v.name, v.ordinal?)))
        .collect::<Vec<_>>()
        .join(", ");
    Some(FieldError {
        field: field.name.clone(),
        code: ValidationCode::TypeMismatch,
        message: format!(
            "field '{}' expected a variant of {} by name or ordinal, one of {accepted}; got {value}",
            field.name, e.name
        ),
    })
}
