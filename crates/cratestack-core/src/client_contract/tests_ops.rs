//! The op list and the digest table.

use super::tests::{attr, field, sample};
use super::*;
use crate::schema::{Schema, TransportStyle};

fn keys(schema: &Schema) -> Vec<String> {
    ops(schema).into_iter().map(|op| op.key).collect()
}

#[test]
fn rpc_ops_are_the_op_ids_the_macro_emits() {
    let mut schema = sample();
    schema.transport = TransportStyle::Rpc;
    assert_eq!(
        keys(&schema),
        [
            "model.Widget.create",
            "model.Widget.delete",
            "model.Widget.get",
            "model.Widget.list",
            "model.Widget.update",
            "procedure.ping",
        ]
    );
}

#[test]
fn rest_ops_are_method_and_route_template() {
    assert_eq!(
        keys(&sample()),
        [
            "DELETE /widgets/{id}",
            "GET /widgets",
            "GET /widgets/{id}",
            "PATCH /widgets/{id}",
            "POST /$procs/ping",
            "POST /widgets",
        ]
    );
}

#[test]
fn an_api_version_is_part_of_a_rest_key() {
    let mut schema = sample();
    schema.procedures[0]
        .attributes
        .push(attr("@api_version(\"v2\")"));
    assert!(keys(&schema).contains(&"POST /v2/$procs/ping".to_owned()));
}

#[test]
fn internal_verbs_are_not_ops_and_subscribe_is() {
    let mut schema = sample();
    schema.transport = TransportStyle::Rpc;
    schema.models[0].attributes = vec![
        attr("@@internal(\"delete\")"),
        attr("@@subscribe"),
        attr("@@emit(created)"),
    ];
    let keys = keys(&schema);
    assert!(!keys.contains(&"model.Widget.delete".to_owned()));
    assert!(keys.contains(&"model.Widget.subscribe".to_owned()));
}

#[test]
fn the_digest_table_is_sorted_and_one_per_op() {
    let table = op_contract_digests(&sample());
    assert_eq!(table.len(), 6);
    assert!(table.windows(2).all(|w| w[0].0 < w[1].0));
    let (key, digest) = &table[0];
    assert_eq!(op_contract_digest(&sample(), key), Some(*digest));
    assert_eq!(op_contract_digest(&sample(), "GET /nope"), None);
}

#[test]
fn equal_shapes_on_different_ops_have_different_digests() {
    let table = op_contract_digests(&sample());
    let mut digests: Vec<_> = table.iter().map(|(_, d)| *d).collect();
    digests.sort();
    digests.dedup();
    assert_eq!(digests.len(), table.len());
}

#[test]
fn an_attribute_off_the_drop_list_moves_the_digest() {
    let mut schema = sample();
    let before = op_contract_digest(&schema, "GET /widgets").unwrap();
    schema.models[0].attributes.push(attr("@@someday(1)"));
    assert_ne!(before, op_contract_digest(&schema, "GET /widgets").unwrap());
}

#[test]
fn the_client_contract_moves_with_any_op() {
    let mut schema = sample();
    let before = client_contract_digest(&schema);
    schema.models[0].fields.push(field("extra", "Int", &[]));
    assert_ne!(before, client_contract_digest(&schema));
}
