//! The rebase invariant: local state == server state + replay(pending).

use serde_json::json;
use uuid::Uuid;

use super::tests_support::{ack, pull, setup, upsert, value};
use crate::protocol::{Change, ChangeOp};

#[test]
fn a_pull_with_no_changes_does_not_replay_pending_work_twice() {
    let (mut conn, engine, _) = setup();
    engine.mutate(&mut conn, "inc", json!({"id": "a"})).unwrap();
    engine.mutate(&mut conn, "inc", json!({"id": "a"})).unwrap();
    assert_eq!(value(&conn, "a"), Some(2));
    engine.apply_pull(&mut conn, pull(vec![])).unwrap();
    assert_eq!(value(&conn, "a"), Some(2));
    assert_eq!(engine.status(&conn).unwrap().pending, 2);
}

#[test]
fn server_changes_land_underneath_pending_mutations() {
    let (mut conn, engine, _) = setup();
    engine.mutate(&mut conn, "inc", json!({"id": "a"})).unwrap();
    // Another client moved the server value to 10 meanwhile.
    engine
        .apply_pull(&mut conn, pull(vec![upsert("a", 10)]))
        .unwrap();
    assert_eq!(value(&conn, "a"), Some(11));
}

#[test]
fn an_ack_in_the_change_stream_retires_pending_mutations() {
    let (mut conn, engine, client_id) = setup();
    engine.mutate(&mut conn, "inc", json!({"id": "a"})).unwrap();
    engine.mutate(&mut conn, "inc", json!({"id": "a"})).unwrap();
    let outcome = engine
        .apply_pull(&mut conn, pull(vec![upsert("a", 2), ack(client_id, 2)]))
        .unwrap();
    assert_eq!(outcome.acked, 2);
    assert_eq!(value(&conn, "a"), Some(2));
    assert_eq!(engine.status(&conn).unwrap().pending, 0);
}

#[test]
fn another_clients_ack_is_ignored() {
    let (mut conn, engine, _) = setup();
    engine.mutate(&mut conn, "inc", json!({"id": "a"})).unwrap();
    engine
        .apply_pull(&mut conn, pull(vec![ack(Uuid::new_v4(), 1)]))
        .unwrap();
    assert_eq!(engine.status(&conn).unwrap().pending, 1);
}

#[test]
fn server_delete_wins_and_a_pending_inc_recreates_the_row() {
    let (mut conn, engine, _) = setup();
    engine
        .apply_pull(&mut conn, pull(vec![upsert("a", 5)]))
        .unwrap();
    engine.mutate(&mut conn, "inc", json!({"id": "a"})).unwrap();
    let delete = Change {
        model: "Counter".into(),
        pk: "a".into(),
        op: ChangeOp::Delete,
    };
    engine.apply_pull(&mut conn, pull(vec![delete])).unwrap();
    // Server state is "gone"; replaying the pending inc inserts value 1.
    assert_eq!(value(&conn, "a"), Some(1));
}
