//! Unit tests for `super` — extracted from an inline `mod tests` block
//! when `idempotent_by_default` pushed `transport.rs` past the
//! workspace's 200-line ceiling (`CLAUDE.md`). Moved verbatim; the
//! `foo.rs` + `foo/tests.rs` shape is this crate's existing one
//! (`batch.rs` + `batch/tests.rs`).

use super::*;

#[test]
fn op_kind_as_str() {
    assert_eq!(OpKind::Unary.as_str(), "unary");
    assert_eq!(OpKind::Sequence.as_str(), "sequence");
    assert_eq!(OpKind::Subscription.as_str(), "subscription");
}

#[test]
fn op_kind_equality() {
    assert_eq!(OpKind::Unary, OpKind::Unary);
    assert_ne!(OpKind::Unary, OpKind::Sequence);
    assert_ne!(OpKind::Sequence, OpKind::Subscription);
}

#[test]
fn canonical_request_string_empty() {
    let result = canonical_request_string("GET", "/api/users", None, None, b"");
    assert_eq!(result, "GET\n/api/users\n\n\n");
}

#[test]
fn canonical_request_string_with_query_and_content_type() {
    let result = canonical_request_string(
        "POST",
        "/api/users",
        Some("id=123"),
        Some("application/json"),
        b"test",
    );
    assert_eq!(
        result,
        "POST\n/api/users\nid=123\napplication/json\n74657374"
    );
}
