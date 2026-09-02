//! Positional-placeholder validation for a `query` block's SQL body
//! (cratestack#867; design `docs/design/declarative-custom-query.md` §2).
//!
//! Two checks, in both directions, because each catches a different real
//! typo:
//!
//! - **Out of range** — the body references `$N` with `N` past the
//!   declared parameter count (or `$0`, which Postgres has no concept of).
//!   Without this, a `$3` typed for `$2` reaches Postgres and fails at
//!   runtime with a bind-count error from inside generated code.
//! - **Declared but never referenced** — a parameter exists in the
//!   signature that no `$N` uses. This is the case the epic's own framing
//!   worried about: writing `$3` instead of `$2` leaves `$2`'s parameter
//!   silently unused, so *both* directions have to be checked or the typo
//!   is only half-caught.
//!
//! The scan itself ([`cratestack_core::scan_sql_placeholders`]) is pure
//! text matching on `$` + digits — no SQL parsing, by design. See that
//! function's doc comment for the one accepted imprecision.

use cratestack_core::{Query, scan_sql_placeholders};

use crate::diagnostics::{SchemaError, span_error};

pub(super) fn validate_query_placeholders(query: &Query) -> Result<(), SchemaError> {
    // A missing `@@sql` body is `validate_query_attributes`' error to
    // report, with a better message than anything this check could give.
    let Some(sql) = query.sql() else {
        return Ok(());
    };

    let referenced = scan_sql_placeholders(sql);
    let declared = u32::try_from(query.args.len()).unwrap_or(u32::MAX);

    if let Some(&out_of_range) = referenced
        .iter()
        .find(|&&index| index == 0 || index > declared)
    {
        return Err(span_error(
            format!(
                "query `{}` references parameter `${out_of_range}` in its SQL body, but {}",
                query.name,
                declared_parameters(query)
            ),
            query.span,
        ));
    }

    for (position, arg) in query.args.iter().enumerate() {
        let index = position as u32 + 1;
        if !referenced.contains(&index) {
            return Err(span_error(
                format!(
                    "query `{}` declares parameter `{}` (`${index}`) but it is never referenced \
                     in the SQL body",
                    query.name, arg.name
                ),
                arg.span,
            ));
        }
    }

    Ok(())
}

/// "only 2 parameter(s) are declared (`userId`, `cutoff`)", or the
/// zero-parameter phrasing, which reads badly with an empty list.
fn declared_parameters(query: &Query) -> String {
    if query.args.is_empty() {
        return "no parameters are declared".to_owned();
    }
    let names = query
        .args
        .iter()
        .map(|arg| format!("`{}`", arg.name))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "only {} parameter(s) are declared ({names})",
        query.args.len()
    )
}
