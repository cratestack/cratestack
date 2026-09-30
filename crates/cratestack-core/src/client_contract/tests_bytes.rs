//! Pins the canonical bytes and literal digests so a derivation or domain-tag
//! change is a reviewed diff, not a silent one. The bytes do not depend on
//! `serde_json`'s `preserve_order` feature: the structs fix the order.

use super::tests::{attr, sample};
use super::*;
use crate::schema::{Schema, TransportStyle};

const PING_JSON: &str = concat!(
    r#"{"closure":{"enums":[],"models":[],"types":[{"fields":[{"attributes":[],"name":"note","#,
    r#""ty":{"arity":"required","generic_args":[],"ident_args":[],"int_args":[],"name":"String"}}],"#,
    r#""name":"Ping"}],"views":[]},"events":null,"key":"POST /$procs/ping","kind":"unary","#,
    r#""model":null,"procedure":{"args":[{"name":"args","ty":{"arity":"required","generic_args":[],"#,
    r#""ident_args":[],"int_args":[],"name":"Ping"}}],"attributes":[],"kind":"mutation","name":"ping","#,
    r#""return_type":{"arity":"required","generic_args":[],"ident_args":[],"int_args":[],"name":"Ping"}},"#,
    r#""transport":"rest","verb":"mutation"}"#,
);

fn hex(schema: &Schema, key: &str) -> String {
    digest_hex(&op_contract_digest(schema, key).expect("op exists"))
}

fn rpc_with_subscribe() -> Schema {
    let mut schema = sample();
    schema.transport = TransportStyle::Rpc;
    schema.models[0].attributes = vec![attr("@@subscribe"), attr("@@emit(created, deleted)")];
    schema
}

#[test]
fn canonical_json_of_a_procedure_is_pinned() {
    assert_eq!(
        op_contract_json(&sample(), "POST /$procs/ping").unwrap(),
        PING_JSON
    );
}

#[test]
fn a_procedure_digest_is_pinned() {
    assert_eq!(
        hex(&sample(), "POST /$procs/ping"),
        "65d376626865420a7652c1a885b6d2fc8fa1ff37b9554a57be2b122109b59640"
    );
}

#[test]
fn a_model_op_digest_is_pinned() {
    assert_eq!(
        hex(&sample(), "GET /widgets"),
        "dd16339991c3ded012ae34ed99523c46a704c270007d44498f00105b104fd1bc"
    );
}

#[test]
fn a_subscribe_op_digest_is_pinned() {
    assert_eq!(
        hex(&rpc_with_subscribe(), "model.Widget.subscribe"),
        "bc32509b1a8631f19ecaf960272ee6f1658c61997cf509aa67f9c27720f25df1"
    );
}

#[test]
fn the_client_contract_digest_is_pinned() {
    assert_eq!(
        digest_hex(&client_contract_digest(&sample())),
        "8a869b88de13fde3f318f06596c067e98a10ee5be2387fd0e9320b344466db88"
    );
}
