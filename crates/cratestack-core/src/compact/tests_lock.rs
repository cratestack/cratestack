//! `add_contract` from the form a `ContractLock` stores.

use serde_json::Value;

use super::tests::{field_node, names};
use super::tests_support::{Parts, field, procedure, schema, type_decl};
use super::{KeyNode, KeyTables};
use crate::client_contract::ContractLock;
use crate::schema::Schema;

fn version(extra: bool) -> Schema {
    let mut fields = vec![field("id", "Uuid", &["@id"]), field("zeta", "String", &[])];
    if extra {
        fields.insert(1, field("alpha", "Int", &[]));
    }
    schema(Parts {
        transport: "rpc",
        models: vec![],
        types: vec![type_decl("Thing", fields)],
        enums: vec![],
        procedures: vec![procedure("make", &[("input", "Thing")], "Thing", &[])],
    })
}

fn digest_of(hex: &str) -> [u8; 32] {
    let mut out = [0; 32];
    for (byte, pair) in out.iter_mut().zip(hex.as_bytes().chunks(2)) {
        *byte = u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap();
    }
    out
}

#[test]
fn a_locked_contract_gets_its_own_table_beside_the_current_one() {
    let old = version(false);
    let new = version(true);
    let mut lock = ContractLock::new();
    assert!(lock.lock_generation(&old, "2026-01-01", "v1").unwrap());

    let mut tables = KeyTables::from_schema(&new);
    for (hex, contract) in &lock.contracts {
        tables.add_contract(digest_of(hex), contract).unwrap();
    }

    let op = "procedure.make";
    let current = crate::op_contract_digest(&new, op).unwrap();
    let locked = crate::op_contract_digest(&old, op).unwrap();
    assert_ne!(current, locked);
    assert_eq!(tables.ops.iter().filter(|k| k.op == op).count(), 2);

    let now = tables.find_digest(op, &current).unwrap();
    let then = tables.find_digest(op, &locked).unwrap();
    let input = |keys: &super::OpKeys| field_node(&tables, &keys.request, "input").clone();
    assert_eq!(names(&tables, &input(now)), ["alpha", "id", "zeta"]);
    // The old client numbered `zeta` 1; the new server numbers it 2.
    assert_eq!(names(&tables, &input(then)), ["id", "zeta"]);
    assert_ne!(now.request, then.request);
    assert!(matches!(now.request, KeyNode::Struct(_)));

    // Adding again replaces, never duplicates, and the artifact keeps both.
    let contract: &Value = lock.contracts.values().next().unwrap();
    tables.add_contract(locked, contract).unwrap();
    assert_eq!(tables.ops.iter().filter(|k| k.op == op).count(), 2);
    assert_eq!(KeyTables::from_bytes(&tables.to_bytes()).unwrap(), tables);
}

#[test]
fn a_model_free_contract_without_closure_is_refused() {
    let mut tables = KeyTables::default();
    let bad = serde_json::json!({"key": "procedure.x"});
    assert!(matches!(
        tables.add_contract([0; 32], &bad),
        Err(super::CompactError::Contract(_))
    ));
    assert!(tables.ops.is_empty());
}
