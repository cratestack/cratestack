//! Push outcomes, registration expiry and the offline queue bound.

use serde_json::json;
use uuid::Uuid;

use super::tests_support::{pull, setup, upsert, value};
use crate::client::{ClientError, Recovery};
use crate::protocol::{
    MutationOutcome, MutationResult, PushResponse, RegisterResponse, RejectCode, SyncErrorBody,
    SyncErrorCode,
};

#[test]
fn a_rejected_mutation_loses_its_local_effect_but_is_kept_for_review() {
    let (mut conn, engine, _) = setup();
    engine
        .apply_pull(&mut conn, pull(vec![upsert("a", 5)]))
        .unwrap();
    engine.mutate(&mut conn, "inc", json!({"id": "a"})).unwrap();
    engine.mutate(&mut conn, "inc", json!({"id": "b"})).unwrap();
    assert_eq!(value(&conn, "a"), Some(6));
    let request = engine.push_request(&conn, 100).unwrap().unwrap();
    assert_eq!(request.mutations.len(), 2);
    let response = PushResponse {
        last_mutation_seq: 2,
        results: vec![
            MutationResult {
                seq: 1,
                outcome: MutationOutcome::Rejected {
                    code: RejectCode::Forbidden,
                    message: "nope".into(),
                },
            },
            MutationResult {
                seq: 2,
                outcome: MutationOutcome::Applied,
            },
        ],
    };
    engine.apply_push(&mut conn, response).unwrap();
    assert_eq!(value(&conn, "a"), Some(5), "rejected inc is undone");
    assert_eq!(
        value(&conn, "b"),
        Some(1),
        "applied inc is kept until acked"
    );
    let status = engine.status(&conn).unwrap();
    assert_eq!((status.pending, status.unpushed), (1, 0));
    assert_eq!(status.rejected[0].code, RejectCode::Forbidden);
    assert!(engine.push_request(&conn, 100).unwrap().is_none());
}

#[test]
fn client_expiry_surfaces_unacknowledged_work_and_renumbers_the_rest() {
    let (mut conn, engine, _) = setup();
    for _ in 0..3 {
        engine.mutate(&mut conn, "inc", json!({"id": "a"})).unwrap();
    }
    let response = PushResponse {
        last_mutation_seq: 1,
        results: vec![MutationResult {
            seq: 1,
            outcome: MutationOutcome::Applied,
        }],
    };
    engine.apply_push(&mut conn, response).unwrap();
    let expired = SyncErrorBody::new(SyncErrorCode::ClientExpired, "gone");
    assert_eq!(
        engine.recover(&mut conn, &expired).unwrap(),
        Recovery::Reregister
    );
    let status = engine.status(&conn).unwrap();
    assert_eq!(status.client_id, None);
    assert_eq!(status.pending, 2);
    assert_eq!(status.rejected[0].code, RejectCode::Expired);
    assert_eq!(value(&conn, "a"), Some(2), "only the two surviving incs");

    let new_id = Uuid::new_v4();
    engine
        .apply_register(
            &mut conn,
            RegisterResponse {
                client_id: new_id,
                mutation_ttl_secs: 60,
            },
        )
        .unwrap();
    let request = engine.push_request(&conn, 100).unwrap().unwrap();
    let seqs: Vec<u64> = request.mutations.iter().map(|m| m.seq).collect();
    assert_eq!(seqs, vec![1, 2]);
    // The resync from zero replaces local state wholesale.
    engine
        .apply_pull(&mut conn, pull(vec![upsert("z", 9)]))
        .unwrap();
    assert_eq!(
        value(&conn, "a"),
        Some(2),
        "pending incs replay on the wiped table"
    );
    assert_eq!(value(&conn, "z"), Some(9));
}

#[test]
fn queue_is_bounded_while_offline() {
    let (mut conn, engine, _) = setup();
    let engine = engine.max_pending(2);
    engine.mutate(&mut conn, "inc", json!({"id": "a"})).unwrap();
    engine.mutate(&mut conn, "inc", json!({"id": "a"})).unwrap();
    assert!(matches!(
        engine.mutate(&mut conn, "inc", json!({"id": "a"})),
        Err(ClientError::QueueFull(2))
    ));
    assert_eq!(
        value(&conn, "a"),
        Some(2),
        "the refused write left no trace"
    );
}

#[test]
fn a_default_engine_accepts_writes() {
    let (mut conn, _, _) = setup();
    // Same registrations as `setup()`, but built through `Default`.
    let engine = crate::client::SyncEngine::default()
        .model(super::tests_support::Counters)
        .mutation("inc", super::tests_support::inc);
    engine.mutate(&mut conn, "inc", json!({"id": "a"})).unwrap();
    assert_eq!(value(&conn, "a"), Some(1));
}
