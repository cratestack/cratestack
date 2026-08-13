//! Generic "can this schema-authored name become a Rust identifier at all"
//! check, shared by every ident site a `.cstack` schema feeds into
//! `cratestack_macros::shared::ident` at codegen time: field names
//! (models/mixins/types/auth/views — see [`super::fields::validate_field_reserved_identifier`]),
//! model/mixin/type/view/procedure names, enum names and variants, and
//! procedure argument names.
//!
//! This generalizes cratestack#398, which only covered fields. Every
//! *other* Rust keyword (`match`, `type`, `ref`, `move`, `impl`, `fn`,
//! `let`, `loop`, `box`, ...) has a valid raw-identifier spelling
//! (`r#type`) and keeps parsing successfully — escaping those happens
//! later, at codegen time, in `cratestack_macros::shared::ident`. Only
//! `self`/`Self`/`super`/`crate` have no valid identifier spelling at all
//! (rustc rejects even the raw form, `r#self`, outright), so those four
//! must be rejected here, at schema-parse time, rather than surfacing as
//! an opaque `rustc` parse error pointing at the `include_server_schema!`
//! macro call site.

use cratestack_core::SourceSpan;

use crate::diagnostics::{SchemaError, span_error};

/// Reject `name` if it has no valid Rust identifier spelling at all.
/// `subject` should read naturally as "$subject cannot be represented..." —
/// e.g. `` field `self` on model `Foo` ``, `` enum `Self` ``, ``
/// procedure argument `crate` on procedure `create` ``.
pub(super) fn validate_reserved_identifier(
    name: &str,
    span: SourceSpan,
    subject: &str,
) -> Result<(), SchemaError> {
    if !cratestack_core::rust_keywords::is_unrepresentable_keyword(name) {
        return Ok(());
    }
    Err(span_error(
        format!(
            "{subject} cannot be represented as a Rust identifier: `{name}` is a reserved \
             keyword with no raw-identifier form (`r#{name}` is not valid Rust). Rename it.",
        ),
        span,
    ))
}
