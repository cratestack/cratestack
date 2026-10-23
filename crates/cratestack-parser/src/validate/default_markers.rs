use cratestack_core::Field;
use cratestack_core::schema::default_markers::{DefaultMarker, MarkerCall};

/// The refusal for a field's `@default(...)` marker call, or `None` when
/// the marker is spelled and used correctly.
///
/// - `dbgenerated()` is a bare marker (matching Prisma's semantics): it
///   asserts the column already has a real database-level default set some
///   other way, and the migration emitter never writes a `DEFAULT` clause
///   for it. Any other spelling (`DbGenerated()`, the bare word), an
///   argument (`dbgenerated("x")`) or text after the call
///   (`dbgenerated() || 'x'`) is refused: the emitter would drop it or write
///   it as SQL no database has.
/// - `autoincrement()` is refused in every form, any case and the bare word
///   included (cratestack#1128). It is not a function in Postgres or SQLite,
///   so no migration could apply it; the marker for "the database generates
///   this value" is `dbgenerated()`.
///
/// Matching goes through `marker_call`, shared with the migration emitter,
/// so spacing and case cannot make the two disagree.
pub(super) fn marker_error(
    model_name: &str,
    field: &Field,
    inner: &str,
    call: MarkerCall<'_>,
) -> Option<String> {
    let message = match call.marker {
        DefaultMarker::Autoincrement => format!(
            "field `{model_name}.{}` uses `@default({inner})`; `autoincrement()` is not a \
             function in Postgres or SQLite, so no migration could apply it \
             (cratestack#1128). If the column's value is generated at the database level (a \
             sequence, `GENERATED ... AS IDENTITY`, a trigger), use the marker \
             `@default(dbgenerated())` and create that sequence or identity yourself in \
             hand-authored migration SQL: cratestack emits neither, and without one an \
             insert that omits the column fails with a NOT NULL violation.",
            field.name,
        ),
        DefaultMarker::DbGenerated if !call.called || !call.canonical => format!(
            "field `{model_name}.{}` uses `@default({inner})`; the marker is spelled exactly \
             `dbgenerated()`. Any other spelling would reach the migration as SQL, and the \
             database has no such function.",
            field.name,
        ),
        DefaultMarker::DbGenerated if !call.args.trim().is_empty() => format!(
            "field `{model_name}.{}` uses `@default(dbgenerated({}))`; cratestack's \
             `dbgenerated()` takes no argument — it is a marker meaning the column already \
             has a real database-level default set some other way (hand-authored migration \
             SQL, a trigger, `GENERATED ... AS IDENTITY`, etc). Remove the argument and use \
             bare `dbgenerated()`.",
            field.name,
            call.args.trim(),
        ),
        DefaultMarker::DbGenerated if !call.is_bare() => format!(
            "field `{model_name}.{}` uses `@default({inner})`; `dbgenerated()` is a marker, \
             not a value, so it must be the whole default expression. Anything combined with \
             it would be dropped from the migration. Use bare `dbgenerated()` and put the \
             real expression in hand-authored migration SQL.",
            field.name,
        ),
        DefaultMarker::DbGenerated => return None,
    };
    Some(message)
}

#[cfg(test)]
#[path = "default_markers_tests.rs"]
mod tests;
