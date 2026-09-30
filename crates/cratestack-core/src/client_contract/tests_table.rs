//! The bound table (`OP_CONTRACTS`) and the route lookup both ends share.

use super::tests::sample;
use super::*;
use crate::schema::TransportStyle;

fn rpc() -> Schema {
    let mut schema = sample();
    schema.transport = TransportStyle::Rpc;
    schema
}

#[test]
fn rest_binds_one_row_per_op_and_no_batch() {
    let table = bound_contracts(&sample());
    assert_eq!(table, op_contract_digests(&sample()));
    assert!(table.iter().all(|(key, _)| key != BATCH_CONTRACT_KEY));
}

#[test]
fn rpc_adds_a_batch_row_holding_the_whole_contract_digest() {
    let schema = rpc();
    let table = bound_contracts(&schema);
    let batch = table.iter().find(|(key, _)| key == BATCH_CONTRACT_KEY);
    assert_eq!(batch.unwrap().1, client_contract_digest(&schema));
    assert_eq!(table.len(), op_contract_digests(&schema).len() + 1);
    let keys: Vec<&str> = table.iter().map(|(key, _)| key.as_str()).collect();
    let mut sorted = keys.clone();
    sorted.sort_unstable();
    assert_eq!(keys, sorted, "sorted by key");
}

#[test]
fn the_batch_row_moves_with_any_op() {
    let mut changed = rpc();
    changed.procedures[0].return_type = super::tests::ty("Int");
    let before = bound_contracts(&rpc());
    let after = bound_contracts(&changed);
    let batch = |table: &[(String, [u8; 32])]| table.iter().find(|(k, _)| k == "batch").unwrap().1;
    assert_ne!(batch(&before), batch(&after));
}

#[test]
fn find_contract_keys_rpc_by_op_id_and_rest_by_method_and_template() {
    let table = [
        ("model.Widget.list", 1),
        ("procedure.ping", 2),
        ("GET /widgets/{id}", 3),
        ("POST /widgets", 4),
    ];
    assert_eq!(find_contract(&table, "POST", "model.Widget.list"), Some(&1));
    assert_eq!(find_contract(&table, "POST", "procedure.ping"), Some(&2));
    assert_eq!(find_contract(&table, "GET", "/widgets/{id}"), Some(&3));
    assert_eq!(find_contract(&table, "POST", "/widgets"), Some(&4));
    // The method is part of a REST key; a HEAD is the GET op's.
    assert_eq!(find_contract(&table, "DELETE", "/widgets/{id}"), None);
    assert_eq!(find_contract(&table, "HEAD", "/widgets/{id}"), Some(&3));
    // A prefix of a key is not the key; a whole key is matched as given
    // (what `ResolvedRoute::with_contract_key` names).
    assert_eq!(find_contract(&table, "POST", "/widget"), None);
    assert_eq!(find_contract(&table, "POST", "GET /widgets/{id}"), Some(&3));
}
