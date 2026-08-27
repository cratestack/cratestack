use crate::diagnostics::SchemaError;

/// Runs one check and records its error instead of propagating it.
///
/// This is what turns the validators from "stop at the first problem" into
/// "report every independent problem". Passing the check as a closure rather
/// than extracting each loop body into a named function is deliberate: the
/// bodies capture half a dozen locals each, and threading those through new
/// signatures would rewrite logic that the parser's existing test suite is the
/// only guard for. Wrapped this way the bodies stay byte-identical.
pub(super) fn record(
    errors: &mut Vec<SchemaError>,
    check: impl FnOnce() -> Result<(), SchemaError>,
) {
    if let Err(error) = check() {
        errors.push(error);
    }
}

/// The first error, in the order the validators ran.
///
/// Every caller that wants the old fail-fast behaviour goes through this, so
/// the collecting path and the single-error path cannot drift: there is one
/// set of checks, run in one order, and this just takes the head of it.
pub(super) fn first(errors: Vec<SchemaError>) -> Result<(), SchemaError> {
    match errors.into_iter().next() {
        Some(error) => Err(error),
        None => Ok(()),
    }
}
