//! Audit snapshots and `@@emit` payloads are `serde_json::to_value(&record)`,
//! so a `BigInt` reaches them as its canonical string and is never a JSON
//! number, which a JavaScript reader would round past 2^53.

use cratestack_core::BigInt;
use serde::Serialize;
use serde_json::json;

use super::BOUNDARIES;
use crate::{primary_key_from_snapshot, snapshot_model};

#[derive(Serialize)]
struct Account {
    id: BigInt,
    balance: Option<BigInt>,
}

#[test]
fn a_bigint_snapshots_as_its_canonical_string_at_every_boundary() {
    for value in BOUNDARIES {
        let snapshot = snapshot_model(&Account {
            id: BigInt::new(value),
            balance: Some(BigInt::new(value)),
        })
        .expect("serializable");
        assert_eq!(snapshot["id"], json!(value.to_string()));
        assert_eq!(snapshot["balance"], json!(value.to_string()));
        assert_eq!(
            primary_key_from_snapshot(&snapshot, "id"),
            json!(value.to_string()),
            "the audit event's primary key stays the canonical string"
        );
    }
}

#[test]
fn a_null_bigint_snapshots_as_null() {
    let snapshot = snapshot_model(&Account {
        id: BigInt::new(1),
        balance: None,
    })
    .expect("serializable");
    assert!(snapshot["balance"].is_null());
}
