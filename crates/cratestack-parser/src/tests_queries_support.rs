//! Shared fixtures for the three `query`-block test modules
//! (cratestack#867): [`crate::tests_queries`] (what a query parses into),
//! [`crate::tests_queries_rejections`] (bad signatures) and
//! [`crate::tests_queries_attributes`] (bad attributes and bad names).
//!
//! Three modules rather than one because of the workspace's 200-line file
//! ceiling; one helper module rather than three copies because
//! [`error_for`]'s "expected schema to be rejected, but it parsed" panic is
//! the guard that stops a rejection test passing for the wrong reason, and
//! a guard worth having is worth having in exactly one place.

use crate::parse_schema;

/// The first error message `source` produces, or a panic naming the real
/// failure — a schema that *parses* when a test expected a rejection is the
/// most important thing these tests can catch, so it must never be
/// reported as a generic assertion failure.
pub(super) fn error_for(source: &str) -> String {
    parse_schema(source)
        .err()
        .map(|error| error.to_string())
        .unwrap_or_else(|| panic!("expected schema to be rejected, but it parsed"))
}

/// Wraps `declaration` in the smallest schema that gives it a result type
/// to name.
pub(super) fn with_query(declaration: &str) -> String {
    format!(
        r#"
type Totals {{
  total Int
}}

{declaration}
"#
    )
}
