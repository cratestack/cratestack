//! `extension webtransport { }` against `extension_gate.rs`'s predicates
//! (slice S5a). Split from `tests.rs` under the 200-LoC convention.

use cratestack_core::ExtensionKind;

#[cfg(feature = "webtransport")]
use super::{feature_enabled, first_missing_extension};

#[cfg(feature = "webtransport")]
fn parse(source: &str) -> cratestack_core::Schema {
    cratestack_parser::parse_schema(source).expect("schema should parse")
}

#[test]
#[cfg(feature = "webtransport")]
fn webtransport_feature_enabled_is_reflected() {
    assert!(feature_enabled(ExtensionKind::Webtransport));
    let schema = parse(
        r#"
transport rpc

extension webtransport {
}

model Widget {
  id Int @id
  @@emit(created)
  @@subscribe
}
"#,
    );
    assert_eq!(first_missing_extension(&schema), None);
}

/// WebTransport is a server binding, not a Postgres capability: the
/// embedded guard rejects it as having nothing to serve, with its own
/// message rather than the Postgres-column one.
#[test]
fn webtransport_is_server_only_not_postgres_only() {
    assert!(super::is_server_only(ExtensionKind::Webtransport));
    assert!(!super::is_postgres_only(ExtensionKind::Webtransport));
    assert!(!super::is_server_only(ExtensionKind::RateLimit));
    assert!(!super::is_server_only(ExtensionKind::Pgvector));
}

/// `extension websocket { }` (slice S5) is a server binding too.
#[test]
fn websocket_is_server_only_not_postgres_only() {
    assert!(super::is_server_only(ExtensionKind::Websocket));
    assert!(!super::is_postgres_only(ExtensionKind::Websocket));
}
