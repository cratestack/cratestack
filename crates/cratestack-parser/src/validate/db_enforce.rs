//! `@db_enforce` on a field that has nothing to enforce (ADR 0019 D5: an
//! attribute either does something or fails `check`).
//!
//! `@db_enforce` makes the validators on the same field a database `CHECK`
//! constraint, and only `@range`, `@length` and `@iso4217` have a SQL form
//! (ADR 0004; `cratestack-migrate/src/convert/checks.rs:23`, the one reader).
//! `@email`, `@uri` and `@regex` are skipped there without a word, so
//! `email String @email @db_enforce` and a bare `x String @db_enforce` both
//! passed `cratestack check` and `migrate diff` emitted no constraint for
//! either: the author believed the column was protected.

use cratestack_core::{Field, field_attribute_name};

use crate::diagnostics::{SchemaError, span_error};

/// Validators that become a `CHECK` constraint under `@db_enforce`.
const ELIGIBLE: [&str; 3] = ["range", "length", "iso4217"];

/// Validators with no SQL form: they run in the application only.
const APPLICATION_ONLY: [&str; 3] = ["email", "uri", "regex"];

pub(super) fn validate_db_enforce(model_name: &str, field: &Field) -> Result<(), SchemaError> {
    let Some(attribute) = field
        .attributes
        .iter()
        .find(|attribute| attribute.raw == "@db_enforce")
    else {
        return Ok(());
    };
    let validators: Vec<&str> = field
        .attributes
        .iter()
        .filter_map(|attribute| field_attribute_name(&attribute.raw))
        .filter(|name| ELIGIBLE.contains(name) || APPLICATION_ONLY.contains(name))
        .collect();
    if validators.iter().any(|name| ELIGIBLE.contains(name)) {
        return Ok(());
    }
    let at = format!("{model_name}.{}", field.name);
    let because = if validators.is_empty() {
        "it has no validator on the field to turn into a constraint".to_owned()
    } else {
        let written = validators
            .iter()
            .map(|name| format!("`@{name}`"))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "the validator{} on it, {written}, has no SQL form",
            plural(validators.len())
        )
    };
    Err(span_error(
        format!(
            "field `{at}` declares `@db_enforce`, but {because}. `@db_enforce` turns `@range`, \
             `@length` or `@iso4217` into a database CHECK constraint; `@email`, `@uri` and \
             `@regex` have no SQL form and run in the application only. `migrate diff` would \
             emit no constraint for this field, so it has no effect and is refused. Add \
             `@range`, `@length` or `@iso4217` to the field, or remove `@db_enforce`",
        ),
        attribute.span,
    ))
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}
