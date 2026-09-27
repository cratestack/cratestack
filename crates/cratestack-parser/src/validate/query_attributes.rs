//! Attribute rules for a `query` block (cratestack#867).
//!
//! 1. Exactly one `@@sql(…)` body — and its argument must actually parse
//!    as a quoted string. Counting the attribute is not enough; see the
//!    inline note below for the three spellings that used to count as a
//!    body while producing none.
//! 2. `@@server_sql`/`@@embedded_sql` are rejected outright: `query` is
//!    Postgres-only and has no per-backend split (design §4), so
//!    accepting the spelling would advertise a backend that does not
//!    exist.
//! 3. Only `@@sql`, `@allow` and `@deny` are recognised, each spelled
//!    exactly as the generator reads it (`super::attribute_shape`), so a
//!    misspelled `@alow` fails loudly instead of silently leaving the
//!    query deny-by-default — and `@deny (…)`, `@deny(…) banned` or
//!    `@Deny(…)` no longer pass while the generator drops the deny rule
//!    (GHSA-69g4-xvcm-vm2j). The name used to be everything before the
//!    first `(`, trimmed, which let all three through.
//!
//! Split from [`super::queries`] for the workspace's 200-line ceiling.

use cratestack_core::{QUERY_SQL_ATTRIBUTE, Query};

use super::attribute_shape::{Arguments, Known, check_shape};
use crate::diagnostics::{SchemaError, span_error};

/// Attributes a `query` block understands, with their readers:
/// `@@sql` — `cratestack_core::Query::sql`; `@allow`/`@deny` —
/// `cratestack-macros/src/query/module.rs`.
const RECOGNISED_ATTRIBUTES: &[Known] = &[
    (QUERY_SQL_ATTRIBUTE, Arguments::Required),
    ("@allow", Arguments::Required),
    ("@deny", Arguments::Required),
];

pub(super) fn validate_query_attributes(query: &Query) -> Result<(), SchemaError> {
    let mut sql_bodies = 0usize;
    let owner = format!("query `{}`", query.name);
    for attribute in &query.attributes {
        let per_backend = ["@@server_sql", "@@embedded_sql"].into_iter().find(|name| {
            attribute.raw.strip_prefix(name).is_some_and(|rest| {
                !rest.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_')
            })
        });
        if let Some(name) = per_backend {
            return Err(span_error(
                format!(
                    "query `{}` declares `{name}`, but a `query` block has no per-backend SQL \
                     split: it is Postgres-only and takes its body from `@@sql(\"…\")`. If this \
                     query needs to run on the embedded backend, it cannot — see \
                     docs/design/declarative-custom-query.md §4",
                    query.name
                ),
                attribute.span,
            ));
        }
        let (name, _) = check_shape(attribute, RECOGNISED_ATTRIBUTES, &owner, "query")?;
        if name == QUERY_SQL_ATTRIBUTE {
            sql_bodies += 1;
        }
    }

    match sql_bodies {
        1 => {}
        0 => {
            return Err(span_error(
                format!(
                    "query `{}` has no SQL body — add `@@sql(\"…\")` (or a `\"\"\"…\"\"\"` block for \
                     multiple lines)",
                    query.name
                ),
                query.span,
            ));
        }
        found => {
            return Err(span_error(
                format!(
                    "query `{}` declares {found} `@@sql` bodies; exactly one is allowed",
                    query.name
                ),
                query.span,
            ));
        }
    }

    // Counting the attribute is not enough: `@@sql(SELECT 1)` (unquoted),
    // a bare `@@sql`, and `@@sql(\"…\") @allow(…)` on one physical line
    // all *count* as one body while `Query::sql()` returns `None` for
    // each. Before cratestack#867's review that combination compiled a
    // query whose `SQL` const was the empty string, with every `$N` check
    // skipped — a schema that looked fine and could never work.
    if query.sql().is_none() {
        return Err(span_error(
            format!(
                "query `{}` has a `@@sql` attribute whose argument is not a quoted string. Write \
                 `@@sql(\"SELECT …\")` for one line, or `@@sql(\"\"\"` … `\"\"\")` for several — and keep \
                 any other attribute (`@allow`, `@deny`) on its own line, since everything up to \
                 the last `)` on the line is read as the SQL argument",
                query.name
            ),
            query.span,
        ));
    }

    Ok(())
}
