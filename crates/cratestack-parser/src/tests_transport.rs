#![cfg(test)]

use super::parse_schema;
use cratestack_core::{Schema, TransportStyle};

/// A minimal serialized `Schema` (JSON, but the shape is format-agnostic —
/// this exercises `#[serde(default)]`, not JSON specifically) with no
/// `"transport"` key at all: what every `Schema` serialized before the
/// `transport` field existed looked like, and — the case this test is
/// actually pinning — what every `Schema` serialized before ticket #170
/// added the `Grpc` variant looked like too. `TransportStyle::Grpc` being
/// additive to an already-`#[serde(default)]` field means old data must
/// keep deserializing unchanged; this is the regression that would fire if
/// that ever stopped being true.
const SCHEMA_JSON_WITHOUT_TRANSPORT_KEY: &str = r#"{
  "datasource": null,
  "auth": null,
  "config_blocks": [],
  "mixins": [],
  "models": [
    {
      "docs": [],
      "name": "Widget",
      "name_span": {"start": 0, "end": 0, "line": 0},
      "fields": [],
      "attributes": [],
      "span": {"start": 0, "end": 0, "line": 0}
    }
  ],
  "types": [],
  "enums": [],
  "procedures": []
}"#;

/// Same, but from after the `transport` field existed and before `Grpc` was
/// added — i.e. every `Schema` a `transport rpc` schema serialized to on
/// disk prior to this ticket.
const SCHEMA_JSON_WITH_RPC_TRANSPORT: &str = r#"{
  "datasource": null,
  "auth": null,
  "config_blocks": [],
  "mixins": [],
  "models": [],
  "types": [],
  "enums": [],
  "procedures": [],
  "transport": "rpc"
}"#;

#[test]
fn schema_without_a_transport_key_still_deserializes_and_defaults_to_rest() {
    let schema: Schema = serde_json::from_str(SCHEMA_JSON_WITHOUT_TRANSPORT_KEY)
        .expect("pre-`transport`-field Schema JSON must still deserialize after adding `Grpc`");
    assert_eq!(schema.transport, TransportStyle::Rest);
    assert_eq!(schema.models.len(), 1);
    assert_eq!(schema.models[0].name, "Widget");
}

#[test]
fn schema_with_rpc_transport_key_still_deserializes_unchanged() {
    let schema: Schema = serde_json::from_str(SCHEMA_JSON_WITH_RPC_TRANSPORT).expect(
        "pre-`Grpc`-variant Schema JSON with `\"transport\":\"rpc\"` must still deserialize",
    );
    assert_eq!(schema.transport, TransportStyle::Rpc);
}

#[test]
fn transport_directive_defaults_to_rest_when_omitted() {
    let schema = parse_schema(
        r#"
model Widget {
  id Int @id
}
"#,
    )
    .expect("schema without transport directive should parse");
    assert_eq!(schema.transport, TransportStyle::Rest);
}

#[test]
fn transport_directive_selects_rpc() {
    let schema = parse_schema(
        r#"
transport rpc

model Widget {
  id Int @id
}
"#,
    )
    .expect("schema with `transport rpc` should parse");
    assert_eq!(schema.transport, TransportStyle::Rpc);
}

#[test]
fn transport_directive_selects_rest_explicitly() {
    let schema = parse_schema(
        r#"
transport rest

model Widget {
  id Int @id
}
"#,
    )
    .expect("schema with `transport rest` should parse");
    assert_eq!(schema.transport, TransportStyle::Rest);
}

#[test]
fn transport_directive_selects_grpc() {
    let schema = parse_schema(
        r#"
transport grpc

model Widget {
  id Int @id
}
"#,
    )
    .expect("schema with `transport grpc` should parse");
    assert_eq!(schema.transport, TransportStyle::Grpc);
}

#[test]
fn transport_directive_rejects_grpc_duplicate() {
    let err = parse_schema(
        r#"
transport grpc
transport rest

model Widget {
  id Int @id
}
"#,
    )
    .expect_err("duplicate transport directive should be rejected even with grpc involved");
    assert!(
        err.to_string().contains("duplicate"),
        "error should mention duplicate, got: {err}",
    );
}

#[test]
fn transport_directive_rejects_unknown_style() {
    let err = parse_schema(
        r#"
transport graphql

model Widget {
  id Int @id
}
"#,
    )
    .expect_err("unknown transport style should be rejected");
    assert!(
        err.to_string().contains("unknown transport style"),
        "error should mention unknown transport style, got: {err}",
    );
}

#[test]
fn transport_directive_rejects_duplicate() {
    let err = parse_schema(
        r#"
transport rpc
transport rest

model Widget {
  id Int @id
}
"#,
    )
    .expect_err("duplicate transport directive should be rejected");
    assert!(
        err.to_string().contains("duplicate"),
        "error should mention duplicate, got: {err}",
    );
}
