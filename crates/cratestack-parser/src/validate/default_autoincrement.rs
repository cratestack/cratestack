use cratestack_core::Field;

use crate::diagnostics::{SchemaError, span_error};

/// Reject `@default(autoincrement(...))`. `autoincrement()` is not a
/// function in Postgres or SQLite, so the migration emitter's pass-through
/// of function-shaped defaults would write `DEFAULT autoincrement()` — DDL
/// both databases refuse at apply time, far from the schema line that
/// caused it (cratestack#1128). The spelling half-works today: `check`
/// accepts it and codegen treats any `@default(...)` as "generated on
/// create", so the failure only surfaces when the first migration is
/// applied. The marker for "the database generates this value" is
/// `@default(dbgenerated())`; point the author there. Any argument is
/// refused too — it would be silently discarded the same way
/// `dbgenerated(<args>)`'s is (see `validate_default_dbgenerated_no_args`
/// in `super::fields`).
pub(super) fn validate_default_autoincrement_rejected(
    model_name: &str,
    field: &Field,
) -> Result<(), SchemaError> {
    let Some(attribute) = field
        .attributes
        .iter()
        .find(|attribute| attribute.raw.starts_with("@default("))
    else {
        return Ok(());
    };
    let Some(inner) = attribute
        .raw
        .strip_prefix("@default(")
        .and_then(|rest| rest.strip_suffix(')'))
    else {
        return Ok(());
    };
    let args = inner
        .trim()
        .strip_prefix("autoincrement(")
        .and_then(|rest| rest.strip_suffix(')'));
    let Some(args) = args else {
        return Ok(());
    };
    let written = if args.trim().is_empty() {
        "autoincrement()".to_owned()
    } else {
        format!("autoincrement({})", args.trim())
    };
    Err(span_error(
        format!(
            "field `{}.{}` uses `@default({written})`; cratestack's `autoincrement()` is not a \
             real database function — Postgres and SQLite both refuse the \
             `DEFAULT autoincrement()` clause the migration emitter would write for it, so the \
             first migration that applies it fails (cratestack#1128). If the column's value is \
             generated at the database level (a sequence, `GENERATED ... AS IDENTITY`, a \
             trigger, hand-authored migration SQL), use the marker `@default(dbgenerated())` \
             instead — it emits no `DEFAULT` clause and asserts exactly that.",
            model_name, field.name,
        ),
        field.span,
    ))
}
