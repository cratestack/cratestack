//! The server half against a real Postgres: change capture, commit-ordered
//! pulls, push replay and acks. TTLs and protections are in
//! `ttl_protection_pg.rs`. Skips without a database unless
//! `CRATESTACK_REQUIRE_DB` is set (see `support/pg.rs`).

mod support;

use chrono::Utc;
use cratestack_sqlx::sqlx;
use cratestack_sync::SyncPolicy;
use cratestack_sync::protocol::{
    ACK_MODEL, ChangeOp, Cursor, Mutation, MutationOutcome, PullRequest, PullResponse, PushRequest,
    RegisterRequest, RejectCode, SyncErrorCode,
};
use cratestack_sync::server::SyncServer;
use serde_json::json;
use support::fixture::{ctx, server};
use support::pg::{connect_or_skip, serial_guard};
use uuid::Uuid;

/// Yields `(pg, guard)`; hold the guard for the whole test. Taken before
/// connecting, so testcontainers start one at a time too.
macro_rules! db_or_skip {
    () => {{
        let guard = serial_guard().await;
        match connect_or_skip().await {
            Some(pg) => (pg, guard),
            None => return,
        }
    }};
}

pub fn mutation(seq: u64, name: &str, args: serde_json::Value) -> Mutation {
    Mutation {
        seq,
        name: name.into(),
        args,
        created_at: Utc::now(),
    }
}

async fn register(server: &SyncServer, user: &str) -> Uuid {
    server
        .register(&ctx(user), RegisterRequest::default())
        .await
        .unwrap()
        .client_id
}

async fn pull(server: &SyncServer, user: &str, client_id: Uuid, cursor: Cursor) -> PullResponse {
    server
        .pull(
            &ctx(user),
            PullRequest {
                client_id,
                cursor,
                limit: None,
            },
        )
        .await
        .unwrap()
}

fn ids(response: &PullResponse) -> Vec<(String, &'static str)> {
    response
        .changes
        .iter()
        .filter(|c| c.model != ACK_MODEL)
        .map(|c| {
            let op = match c.op {
                ChangeOp::Upsert { .. } => "upsert",
                ChangeOp::Delete => "delete",
            };
            (c.pk.clone(), op)
        })
        .collect()
}

#[tokio::test]
async fn writes_from_any_path_reach_only_their_owners_scope() {
    let (pg, _serial) = db_or_skip!();
    let pool = pg.fresh_database().await;
    let server = server(&pool, SyncPolicy::default()).await;
    // Plain SQL — not a sync push — is captured by the trigger.
    sqlx::query(
        "INSERT INTO todos (id, owner, title) VALUES ('a1', 'alice', 'x'), ('b1', 'bob', 'y')",
    )
    .execute(&pool)
    .await
    .unwrap();
    let alice = register(&server, "alice").await;
    let bob = register(&server, "bob").await;
    let page = pull(&server, "alice", alice, Cursor::ZERO).await;
    assert_eq!(ids(&page), vec![("a1".into(), "upsert")]);
    let page = pull(&server, "bob", bob, Cursor::ZERO).await;
    assert_eq!(ids(&page), vec![("b1".into(), "upsert")]);
}

#[tokio::test]
async fn push_applies_retries_are_acknowledged_not_reapplied_and_acks_ride_the_stream() {
    let (pg, _serial) = db_or_skip!();
    let pool = pg.fresh_database().await;
    let server = server(&pool, SyncPolicy::default()).await;
    let client_id = register(&server, "alice").await;
    let request = PushRequest {
        client_id,
        mutations: vec![
            mutation(1, "todo.create", json!({"id": "t1", "title": "one"})),
            mutation(2, "todo.create", json!({"id": "t2", "title": "two"})),
        ],
    };
    let first = server.push(&ctx("alice"), request.clone()).await.unwrap();
    assert_eq!(first.last_mutation_seq, 2);
    assert!(
        first
            .results
            .iter()
            .all(|r| r.outcome == MutationOutcome::Applied)
    );
    // Same push again (the response was "lost"): nothing re-executes —
    // a second INSERT of t1 would have failed on the primary key.
    let retry = server.push(&ctx("alice"), request).await.unwrap();
    assert!(
        retry
            .results
            .iter()
            .all(|r| r.outcome == MutationOutcome::AlreadyApplied)
    );

    let page = pull(&server, "alice", client_id, Cursor::ZERO).await;
    assert_eq!(
        ids(&page),
        vec![("t1".into(), "upsert"), ("t2".into(), "upsert")]
    );
    let ack = page.changes.iter().find(|c| c.model == ACK_MODEL).unwrap();
    assert_eq!(
        ack.op,
        ChangeOp::Upsert {
            row: json!({"seq": 2})
        }
    );
}

#[tokio::test]
async fn a_rejected_mutation_rolls_back_only_its_own_writes() {
    let (pg, _serial) = db_or_skip!();
    let pool = pg.fresh_database().await;
    let server = server(&pool, SyncPolicy::default()).await;
    let client_id = register(&server, "alice").await;
    let response = server
        .push(
            &ctx("alice"),
            PushRequest {
                client_id,
                mutations: vec![
                    mutation(1, "todo.create", json!({"id": "a", "title": "ok"})),
                    mutation(2, "todo.create", json!({"id": "b", "title": "boom"})),
                    mutation(3, "todo.rename", json!({"id": "nope", "title": "?"})),
                    mutation(4, "todo.explode", json!({})),
                    mutation(5, "todo.create", json!({"id": "c", "title": "ok"})),
                ],
            },
        )
        .await
        .unwrap();
    let codes: Vec<Option<RejectCode>> = response
        .results
        .iter()
        .map(|r| match &r.outcome {
            MutationOutcome::Rejected { code, .. } => Some(*code),
            _ => None,
        })
        .collect();
    assert_eq!(
        codes,
        vec![
            None,
            Some(RejectCode::Invalid),
            Some(RejectCode::Forbidden),
            Some(RejectCode::UnknownMutation),
            None
        ]
    );
    assert_eq!(
        response.last_mutation_seq, 5,
        "rejections consume their seq"
    );
    let rows: Vec<String> = sqlx::query_scalar("SELECT id FROM todos ORDER BY id")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(rows, vec!["a", "c"], "the savepoint discarded b's insert");
}

#[tokio::test]
async fn a_gap_in_the_sequence_is_refused_with_the_servers_position() {
    let (pg, _serial) = db_or_skip!();
    let pool = pg.fresh_database().await;
    let server = server(&pool, SyncPolicy::default()).await;
    let client_id = register(&server, "alice").await;
    let error = server
        .push(
            &ctx("alice"),
            PushRequest {
                client_id,
                mutations: vec![mutation(2, "todo.create", json!({"id": "a", "title": "x"}))],
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, SyncErrorCode::SequenceGap);
    assert_eq!(error.last_mutation_seq, Some(0));
}

/// The claim the cursor design rests on. Transaction A takes the earlier
/// position but commits after B. A cursor that moved past B while A was in
/// flight would never see A; the xmin gate holds B back until A is done.
#[tokio::test]
async fn a_transaction_that_commits_late_is_not_skipped() {
    let (pg, _serial) = db_or_skip!();
    let pool = pg.fresh_database().await;
    let server = server(&pool, SyncPolicy::default()).await;
    let client_id = register(&server, "alice").await;

    let mut slow = pool.begin().await.unwrap();
    sqlx::query("INSERT INTO todos (id, owner, title) VALUES ('early', 'alice', 'A')")
        .execute(&mut *slow)
        .await
        .unwrap();
    sqlx::query("INSERT INTO todos (id, owner, title) VALUES ('late', 'alice', 'B')")
        .execute(&pool)
        .await
        .unwrap();

    let held = pull(&server, "alice", client_id, Cursor::ZERO).await;
    assert!(ids(&held).is_empty(), "B must wait for A: {:?}", ids(&held));
    assert_eq!(held.cursor, Cursor::ZERO);

    slow.commit().await.unwrap();
    let released = pull(&server, "alice", client_id, held.cursor).await;
    assert_eq!(
        ids(&released),
        vec![("early".into(), "upsert"), ("late".into(), "upsert")]
    );
}

#[tokio::test]
async fn leaving_a_scope_or_becoming_unreadable_arrives_as_a_delete() {
    let (pg, _serial) = db_or_skip!();
    let pool = pg.fresh_database().await;
    let server = server(&pool, SyncPolicy::default()).await;
    sqlx::query(
        "INSERT INTO todos (id, owner, title) VALUES ('m', 'alice', 'x'), ('s', 'alice', 'y')",
    )
    .execute(&pool)
    .await
    .unwrap();
    let alice = register(&server, "alice").await;
    let bob = register(&server, "bob").await;
    let start = pull(&server, "alice", alice, Cursor::ZERO).await;

    sqlx::query("UPDATE todos SET owner = 'bob' WHERE id = 'm'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE todos SET deleted = TRUE WHERE id = 's'")
        .execute(&pool)
        .await
        .unwrap();
    let page = pull(&server, "alice", alice, start.cursor).await;
    assert_eq!(
        ids(&page),
        vec![("m".into(), "delete"), ("s".into(), "delete")]
    );
    let page = pull(&server, "bob", bob, Cursor::ZERO).await;
    assert_eq!(ids(&page), vec![("m".into(), "upsert")]);
}

#[tokio::test]
async fn pages_are_bounded_and_resume_from_the_cursor() {
    let (pg, _serial) = db_or_skip!();
    let pool = pg.fresh_database().await;
    let server = server(&pool, SyncPolicy::default()).await;
    for i in 0..5 {
        sqlx::query("INSERT INTO todos (id, owner, title) VALUES ($1, 'alice', 'x')")
            .bind(format!("t{i}"))
            .execute(&pool)
            .await
            .unwrap();
    }
    let client_id = register(&server, "alice").await;
    let mut cursor = Cursor::ZERO;
    let mut seen = Vec::new();
    loop {
        let page = server
            .pull(
                &ctx("alice"),
                PullRequest {
                    client_id,
                    cursor,
                    limit: Some(2),
                },
            )
            .await
            .unwrap();
        assert!(page.changes.len() <= 2);
        seen.extend(ids(&page).into_iter().map(|(id, _)| id));
        cursor = page.cursor;
        if !page.has_more {
            break;
        }
    }
    assert_eq!(seen, vec!["t0", "t1", "t2", "t3", "t4"]);
}
