//! The lock: locking, integrity, the accepted table, pruning.

use serde_json::json;

use super::tests::field;
use super::tests_compat::*;
use super::{
    ContractLock, LockError, bound_contracts, client_contract_digest, digest_hex,
    op_contract_digest,
};

const CREATE: &str = "model.Widget.create";
const PAINT: &str = "procedure.paint";

fn locked(schema: &crate::schema::Schema, date: &str, note: &str) -> ContractLock {
    let mut lock = ContractLock::new();
    assert!(lock.lock_generation(schema, date, note).unwrap());
    lock
}

fn widened() -> crate::schema::Schema {
    let mut s = base();
    s.models[0]
        .fields
        .push(optional(field("note", "String", &[])));
    s
}

#[test]
fn locking_is_idempotent_and_the_file_round_trips() {
    let mut lock = locked(&base(), "2026-10-01", "store 1.0");
    assert!(
        !lock
            .lock_generation(&base(), "2026-10-09", "again")
            .unwrap()
    );
    assert_eq!(lock.generations.len(), 1);
    assert!(lock.is_locked(&base()));
    assert!(!lock.is_locked(&widened()));
    let text = lock.to_json();
    assert!(text.ends_with('\n'));
    assert_eq!(ContractLock::parse(&text).unwrap(), lock);
    assert_eq!(
        lock.generations[0].client_contract,
        digest_hex(&client_contract_digest(&base()))
    );
}

#[test]
fn a_contract_is_stored_once_however_many_generations_carry_it() {
    let mut lock = locked(&base(), "2026-10-01", "");
    let first = lock.contracts.len();
    assert!(lock.lock_generation(&widened(), "2026-10-02", "").unwrap());
    // Only the model's ops moved; the procedures' contracts are shared.
    let moved = lock.generations[1]
        .ops
        .iter()
        .filter(|(op, hex)| lock.generations[0].ops[*op] != **hex)
        .count();
    assert_eq!(lock.contracts.len(), first + moved);
    assert!(moved > 0 && moved < lock.generations[1].ops.len());
}

#[test]
fn the_accepted_table_is_current_then_locked_newest_first() {
    let mut lock = locked(&base(), "2026-10-01", "a");
    let mut mid = base();
    mid.models[0]
        .fields
        .push(optional(field("note", "String", &[])));
    lock.lock_generation(&mid, "2026-10-02", "b").unwrap();
    let mut now = mid.clone();
    now.models[0]
        .fields
        .push(optional(field("tag", "String", &[])));
    let table = lock.accepted(&now).unwrap();
    let row = |key: &str| &table.iter().find(|(k, _)| k == key).unwrap().1;
    let digest = |s: &crate::schema::Schema| op_contract_digest(s, CREATE).unwrap();
    assert_eq!(
        row(CREATE),
        &vec![digest(&now), digest(&mid), digest(&base())]
    );
    // An op that did not move has its one digest, not a copy per generation.
    assert_eq!(row(PAINT).len(), 1);
    assert_eq!(
        table.len(),
        bound_contracts(&now).len(),
        "a row per bound key, batch included"
    );
    assert_eq!(row("batch").len(), 1, "batch never takes history");
    assert!(
        lock.generations
            .iter()
            .all(|g| !g.ops.contains_key("batch"))
    );
}

#[test]
fn an_incompatible_entry_is_refused_naming_the_op_and_the_reason() {
    let lock = locked(&base(), "2026-10-01", "");
    let mut now = base();
    now.models[0].fields.pop();
    let error = lock.accepted(&now).unwrap_err();
    let LockError::Incompatible(broken) = &error else {
        panic!("{error:?}");
    };
    assert!(broken.iter().any(|b| b.op == CREATE));
    assert!(
        broken.iter().all(|b| b.op.starts_with("model.Widget.")),
        "{broken:?}"
    );
    let message = error.to_string();
    assert!(message.contains("`Widget.name` was removed"), "{message}");
    assert!(
        message.contains("contract prune --op model.Widget.create"),
        "{message}"
    );
    // Pruning the broken ops is the deliberate way through.
    let mut lock = lock;
    for op in broken {
        lock.prune_op(&op.op);
    }
    let table = lock.accepted(&now).unwrap();
    assert!(table.iter().all(|(_, digests)| digests.len() == 1));
}

#[test]
fn an_op_the_schema_no_longer_has_is_not_judged_or_accepted() {
    let lock = locked(&base(), "2026-10-01", "");
    let mut now = base();
    now.procedures.retain(|p| p.name != "ping");
    let table = lock.accepted(&now).unwrap();
    assert!(table.iter().all(|(key, _)| key != "procedure.ping"));
}

#[test]
fn a_hand_edited_lock_is_refused() {
    let text = locked(&base(), "2026-10-01", "").to_json();
    let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
    let key = value["contracts"]
        .as_object()
        .unwrap()
        .keys()
        .next()
        .unwrap()
        .clone();
    value["contracts"][&key]["verb"] = json!("tampered");
    let error = ContractLock::parse(&value.to_string()).unwrap_err();
    assert!(matches!(error, LockError::Integrity { .. }), "{error}");

    let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
    value["generations"][0]["ops"]["procedure.ping"] = json!("0".repeat(64));
    assert!(matches!(
        ContractLock::parse(&value.to_string()).unwrap_err(),
        LockError::Dangling { .. }
    ));
    for (path, bad) in [("format", json!(2)), ("domain", json!("x"))] {
        let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
        value[path] = bad;
        assert!(ContractLock::parse(&value.to_string()).is_err(), "{path}");
    }
    assert!(ContractLock::parse("{\"format\":1,\"extra\":1}").is_err());
}

#[test]
fn prune_drops_history_and_the_contracts_only_it_held() {
    let mut lock = locked(&base(), "2026-10-01", "a");
    lock.lock_generation(&widened(), "2026-10-05", "b").unwrap();
    let mut newest = widened();
    newest.models[0]
        .fields
        .push(optional(field("tag", "String", &[])));
    lock.lock_generation(&newest, "2026-10-09", "c").unwrap();

    let mut by_date = lock.clone();
    assert_eq!(by_date.prune_before("2026-10-05").unwrap().generations, 1);
    assert_eq!(by_date.generations.len(), 2);

    let mut keep = lock.clone();
    assert_eq!(keep.prune_keep(1).generations, 2);
    assert_eq!(keep.generations[0].note, "c");
    assert!(keep.contracts.len() < lock.contracts.len());
    assert!(ContractLock::parse(&keep.to_json()).is_ok());

    let mut one = lock.clone();
    let hex = one.generations[1].client_contract.clone();
    assert_eq!(one.prune_generation(&hex[..10]).unwrap().generations, 1);
    assert!(one.prune_generation(&hex[..10]).is_err(), "gone");
    assert!(one.prune_generation(&hex[..4]).is_err(), "too short");

    let mut op = lock.clone();
    let pruned = op.prune_op(CREATE);
    assert_eq!(pruned.entries, 3);
    assert!(op.generations.iter().all(|g| !g.ops.contains_key(CREATE)));
    assert!(pruned.contracts >= 3);
}
