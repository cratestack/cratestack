#![cfg(test)]

use super::parse_schema;
use cratestack_core::TransportStyle;

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
