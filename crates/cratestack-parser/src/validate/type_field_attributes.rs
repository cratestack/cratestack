//! What a field of a `type` may not carry, each with its reason, ahead of
//! the closed list (`super::field_attribute_tables`) so the author is told
//! what to do instead and not only that the name is unknown.
//!
//! A `type` has no table and no create input, which decides three things
//! that a model field would allow (ADR 0019 D5, PR A):
//!
//! - `@default` fills a model's create input. A `type` is decoded exactly as
//!   sent, so a missing field is a decode error and nothing applies the
//!   default (`cratestack-api/tests/contract_roundtrip.rs` pins that, and the
//!   contract classifier says the same,
//!   `cratestack-core/src/client_contract/compat_decl.rs`).
//! - `@db_enforce` makes a validator a `CHECK` constraint, and a `type` has
//!   no table. The validator still runs on a procedure argument without it.
//! - A validator checks one value. On a list field it would have to mean the
//!   list or each element, and the generated check does neither; a `type`
//!   with the validated field, taken as a list, does the element-wise check.

use cratestack_core::{Field, TypeArity, field_attribute_name};

use crate::diagnostics::{SchemaError, span_error};

const VALIDATORS: [&str; 6] = ["length", "range", "regex", "email", "uri", "iso4217"];

pub(super) fn validate_type_field(type_name: &str, field: &Field) -> Result<(), SchemaError> {
    let at = format!("{type_name}.{}", field.name);
    for attribute in &field.attributes {
        let raw = attribute.raw.as_str();
        let name = field_attribute_name(raw).unwrap_or_default();
        let message = if name == "default" {
            format!(
                "field `{at}` declares `{raw}`, but `{type_name}` is a `type`: a `type` is decoded \
                 as the client sent it, a missing field is a decode error, and nothing applies a \
                 default (`@default` fills a model's create input), so it has no effect and is \
                 refused. Make the field optional (`{} {}?`), or remove `@default`",
                field.name, field.ty.name,
            )
        } else if name == "db_enforce" {
            format!(
                "field `{at}` declares `{raw}`, but `{type_name}` is a `type`, which has no table: \
                 `@db_enforce` turns a validator into a database CHECK constraint, so it has no \
                 effect here and is refused. The validators on this field still run on a procedure \
                 argument without it. Remove `@db_enforce`",
            )
        } else if VALIDATORS.contains(&name) && field.ty.arity == TypeArity::List {
            format!(
                "field `{at}` is a list (`{}[]`) and declares `{raw}`: a validator checks one \
                 value, and a list is not checked element by element, so nothing would be \
                 validated. It is refused. Declare a `type` whose field carries the validator and \
                 take a list of that `type`, or remove the validator",
                field.ty.name,
            )
        } else {
            continue;
        };
        return Err(span_error(message, attribute.span));
    }
    Ok(())
}
