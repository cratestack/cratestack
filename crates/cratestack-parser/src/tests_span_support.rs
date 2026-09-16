//! Shared fixture helpers for the span and reserved-keyword test modules.
//!
//! The assertions are what those tests are about; the fallible steps that lead
//! up to one — parsing a fixture, locating a declaration inside it — are not.
//! Routing them through here gives each failure mode a single message that
//! names the fixture, instead of a bare `unwrap` panic repeated at every call
//! site.

use cratestack_core::Schema;

use crate::SchemaError;
use crate::parse_schema;

/// Parse a fixture that is expected to be valid, reporting the fixture itself
/// when it is not.
pub(crate) fn parse_ok(source: &str) -> Schema {
    match parse_schema(source) {
        Ok(schema) => schema,
        Err(error) => panic!("expected {source:?} to parse, got: {error}"),
    }
}

/// Parse a fixture that is expected to be rejected.
pub(crate) fn parse_err(source: &str) -> SchemaError {
    parse_schema(source).expect_err("expected the fixture to be rejected")
}

/// `Option::unwrap` with the subject named — for values a fixture has already
/// established are present.
pub(crate) fn present<T>(value: Option<T>, subject: &str) -> T {
    match value {
        Some(value) => value,
        None => panic!("expected {subject} to be present"),
    }
}

/// Byte offset of `needle` in `source`.
pub(crate) fn offset_of(source: &str, needle: &str) -> usize {
    present(source.find(needle), &format!("{needle:?} in {source:?}"))
}
