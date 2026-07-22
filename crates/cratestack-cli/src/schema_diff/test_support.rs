#![cfg(test)]
//! Shared helpers for the `schema_diff` test suites.

use super::{SchemaDiff, Severity, diff_schemas};

fn parse(source: &str) -> cratestack_core::Schema {
    cratestack_parser::parse_schema(source).expect("schema should parse")
}

pub(super) fn diff(prev: &str, next: &str) -> SchemaDiff {
    diff_schemas(&parse(prev), &parse(next))
}

pub(super) fn categories(diff: &SchemaDiff, severity: Severity) -> Vec<&'static str> {
    diff.changes
        .iter()
        .filter(|change| change.severity == severity)
        .map(|change| change.category)
        .collect()
}
