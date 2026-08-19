#![cfg(test)]

use super::parse_schema;
use cratestack_core::{Schema, TransportStyle};

/// A minimal serialized `Schema` (JSON, but the shape is format-agnostic —
/// this exercises `#[serde(default)]`, not JSON specifically) with no
/// `"transport"` key at all: what every `Schema` serialized before the
/// `transport` field existed looked like. `transport` being
/// `#[serde(default)]` means old data must keep deserializing unchanged;
/// this is the regression that would fire if that ever stopped being true.
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

/// Same, but from after the `transport` field existed — i.e. every `Schema`
/// a `transport rpc` schema has serialized to on disk.
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
        .expect("pre-`transport`-field Schema JSON must still deserialize");
    assert_eq!(schema.transport, TransportStyle::Rest);
    assert_eq!(schema.models.len(), 1);
    assert_eq!(schema.models[0].name, "Widget");
}

#[test]
fn schema_with_rpc_transport_key_still_deserializes_unchanged() {
    let schema: Schema = serde_json::from_str(SCHEMA_JSON_WITH_RPC_TRANSPORT)
        .expect("Schema JSON with `\"transport\":\"rpc\"` must still deserialize");
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

/// gRPC support was removed (v0.9 breaking change): `transport grpc` must
/// no longer parse at all. This is the decisive regression guard — it
/// fails the moment `"grpc"` is re-added to
/// `parse_transport_directive`'s match arms.
#[test]
fn transport_directive_rejects_grpc() {
    let err = parse_schema(
        r#"
transport grpc

model Widget {
  id Int @id
}
"#,
    )
    .expect_err("`transport grpc` should no longer parse");
    let message = err.to_string();
    assert!(
        message.contains("no longer supported"),
        "error should say the transport was removed, not merely that it is \
         unrecognised, got: {message}",
    );
    assert!(
        message.contains("removed in v0.9"),
        "error should name the release that removed it, got: {message}",
    );
    assert!(
        message.contains("transport rest") && message.contains("transport rpc"),
        "error should point at the surviving transports as the migration \
         target, got: {message}",
    );
}

/// The removal message is reserved for `grpc` specifically — an actual typo
/// or an unimplemented transport must still get the generic
/// unknown-style error, not a misleading "was removed in v0.9" claim about
/// something that never existed.
#[test]
fn transport_directive_rejects_grpc_distinctly_from_a_typo() {
    let typo = parse_schema(
        r#"
transport graphql

model Widget {
  id Int @id
}
"#,
    )
    .expect_err("`transport graphql` should not parse")
    .to_string();

    assert!(
        typo.contains("unknown transport style `graphql`"),
        "got: {typo}",
    );
    assert!(
        !typo.contains("removed in v0.9"),
        "a transport that never existed must not be described as removed, got: {typo}",
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
