//! TTLs and protections against a real Postgres.

mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use async_trait::async_trait;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use chrono::{TimeDelta, Utc};
use cratestack_core::{
    CratestackContext, CratestackError, RateLimitConfig, RateLimitDecision, RateLimitStore,
};
use cratestack_sqlx::sqlx;
use cratestack_sync::SyncPolicy;
use cratestack_sync::protocol::{
    Cursor, Mutation, MutationOutcome, PullRequest, PushRequest, RegisterRequest, RejectCode,
    SyncErrorBody, SyncErrorCode,
};
use cratestack_sync::server::{SyncServer, sync_router};
use serde_json::json;
use support::fixture::{ctx, resolver, server};
use support::pg::{connect_or_skip, serial_guard};
use tower::ServiceExt;
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

fn create(seq: u64, id: &str, age: TimeDelta) -> Mutation {
    Mutation {
        seq,
        name: "todo.create".into(),
        args: json!({ "id": id, "title": "t" }),
        created_at: Utc::now() - age,
    }
}

async fn register(server: &SyncServer, user: &str) -> Result<Uuid, SyncErrorBody> {
    Ok(server
        .register(&ctx(user), RegisterRequest::default())
        .await?
        .client_id)
}

async fn pull(
    server: &SyncServer,
    user: &str,
    id: Uuid,
    cursor: Cursor,
) -> Result<Cursor, SyncErrorBody> {
    let request = PullRequest {
        client_id: id,
        cursor,
        limit: None,
    };
    Ok(server.pull(&ctx(user), request).await?.cursor)
}

#[tokio::test]
async fn mutation_ttl_rejects_stale_intent_and_post_dated_mutations() {
    let (pg, _serial) = db_or_skip!();
    let pool = pg.fresh_database().await;
    let server = server(&pool, SyncPolicy::default()).await;
    let client_id = register(&server, "alice").await.unwrap();
    let mutations = vec![
        create(1, "stale", TimeDelta::days(8)),
        create(2, "future", TimeDelta::hours(-1)),
        create(3, "fresh", TimeDelta::zero()),
    ];
    let response = server
        .push(
            &ctx("alice"),
            PushRequest {
                client_id,
                mutations,
            },
        )
        .await
        .unwrap();
    let codes: Vec<_> = response
        .results
        .iter()
        .map(|r| match &r.outcome {
            MutationOutcome::Rejected { code, .. } => Some(*code),
            _ => None,
        })
        .collect();
    assert_eq!(
        codes,
        vec![Some(RejectCode::Expired), Some(RejectCode::Invalid), None]
    );
}

#[tokio::test]
async fn tombstone_ttl_expires_cursors_that_could_have_missed_a_delete() {
    let (pg, _serial) = db_or_skip!();
    let pool = pg.fresh_database().await;
    let policy = SyncPolicy {
        tombstone_ttl_secs: 1,
        ..SyncPolicy::default()
    };
    let server = server(&pool, policy).await;
    sqlx::query("INSERT INTO todos (id, owner, title) VALUES ('t', 'alice', 'x')")
        .execute(&pool)
        .await
        .unwrap();
    let client_id = register(&server, "alice").await.unwrap();
    let before_delete = pull(&server, "alice", client_id, Cursor::ZERO)
        .await
        .unwrap();
    sqlx::query("DELETE FROM todos WHERE id = 't'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE cratestack_sync_changes SET changed_at = now() - interval '1 hour'")
        .execute(&pool)
        .await
        .unwrap();

    let report = server.gc().await.unwrap();
    assert_eq!(report.tombstones_purged, 1);
    let error = pull(&server, "alice", client_id, before_delete)
        .await
        .unwrap_err();
    assert_eq!(error.code, SyncErrorCode::CursorExpired);
    // A full resync from zero is always allowed.
    pull(&server, "alice", client_id, Cursor::ZERO)
        .await
        .unwrap();
}

#[tokio::test]
async fn client_ttl_evicts_idle_clients_and_their_acks() {
    let (pg, _serial) = db_or_skip!();
    let pool = pg.fresh_database().await;
    let server = server(&pool, SyncPolicy::default()).await;
    let client_id = register(&server, "alice").await.unwrap();
    server
        .push(
            &ctx("alice"),
            PushRequest {
                client_id,
                mutations: vec![create(1, "a", TimeDelta::zero())],
            },
        )
        .await
        .unwrap();
    sqlx::query("UPDATE cratestack_sync_clients SET last_seen_at = now() - interval '31 days'")
        .execute(&pool)
        .await
        .unwrap();

    assert_eq!(server.gc().await.unwrap().clients_evicted, 1);
    let acks: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM cratestack_sync_changes WHERE scope LIKE 'client:%'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(acks, 0);
    let error = pull(&server, "alice", client_id, Cursor::ZERO)
        .await
        .unwrap_err();
    assert_eq!(error.code, SyncErrorCode::ClientExpired);
}

#[tokio::test]
async fn a_client_id_is_useless_to_another_principal() {
    let (pg, _serial) = db_or_skip!();
    let pool = pg.fresh_database().await;
    let server = server(&pool, SyncPolicy::default()).await;
    let alices = register(&server, "alice").await.unwrap();
    let push = server
        .push(
            &ctx("bob"),
            PushRequest {
                client_id: alices,
                mutations: vec![create(1, "x", TimeDelta::zero())],
            },
        )
        .await
        .unwrap_err();
    assert_eq!(push.code, SyncErrorCode::ClientMismatch);
    let pulled = pull(&server, "bob", alices, Cursor::ZERO)
        .await
        .unwrap_err();
    assert_eq!(pulled.code, SyncErrorCode::ClientMismatch);
}

#[tokio::test]
async fn registrations_and_pushes_are_bounded() {
    let (pg, _serial) = db_or_skip!();
    let pool = pg.fresh_database().await;
    let policy = SyncPolicy {
        max_clients_per_principal: 2,
        max_mutations_per_push: 2,
        max_mutation_bytes: 64,
        ..SyncPolicy::default()
    };
    let server = server(&pool, policy).await;
    let client_id = register(&server, "alice").await.unwrap();
    register(&server, "alice").await.unwrap();
    let third = register(&server, "alice").await.unwrap_err();
    assert_eq!(third.code, SyncErrorCode::TooManyClients);
    register(&server, "bob")
        .await
        .expect("the cap is per principal");

    let many = (1..=3)
        .map(|seq| create(seq, "x", TimeDelta::zero()))
        .collect();
    let error = server
        .push(
            &ctx("alice"),
            PushRequest {
                client_id,
                mutations: many,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, SyncErrorCode::TooManyMutations);
    let mut big = create(1, "x", TimeDelta::zero());
    big.args = json!({ "id": "x", "title": "y".repeat(100) });
    let error = server
        .push(
            &ctx("alice"),
            PushRequest {
                client_id,
                mutations: vec![big],
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, SyncErrorCode::MutationTooLarge);
}

/// Allows `n` calls in total, then throttles. Stands in for the in-memory
/// and Redis stores the builder accepts.
struct Budget(AtomicU32);

#[async_trait]
impl RateLimitStore for Budget {
    async fn consume(
        &self,
        _key: &str,
        _config: RateLimitConfig,
    ) -> Result<RateLimitDecision, CratestackError> {
        Ok(match self.0.fetch_sub(1, Ordering::SeqCst) {
            0 => RateLimitDecision::Throttled {
                retry_after_secs: 7,
            },
            left => RateLimitDecision::Allowed {
                remaining: left - 1,
            },
        })
    }
}

#[tokio::test]
async fn the_http_surface_authenticates_rate_limits_and_maps_status_codes() {
    let (pg, _serial) = db_or_skip!();
    let pool = pg.fresh_database().await;
    let base = server(&pool, SyncPolicy::default()).await;
    let server = SyncServer::builder(pool.clone(), Default::default(), resolver)
        .rate_limit(
            Arc::new(Budget(AtomicU32::new(1))),
            RateLimitConfig::new(1, 1.0),
        )
        .build()
        .unwrap();
    drop(base);
    let auth = |headers: &axum::http::HeaderMap| -> Result<CratestackContext, CratestackError> {
        Ok(match headers.get("x-user").and_then(|v| v.to_str().ok()) {
            Some(user) => ctx(user),
            None => CratestackContext::anonymous(),
        })
    };
    let app = sync_router(server, auth);
    let call = |user: Option<&'static str>| {
        let mut request =
            Request::post("/sync/register").header("content-type", "application/json");
        if let Some(user) = user {
            request = request.header("x-user", user);
        }
        app.clone().oneshot(request.body(Body::from("{}")).unwrap())
    };

    let anonymous = call(None).await.unwrap();
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(call(Some("alice")).await.unwrap().status(), StatusCode::OK);
    let throttled = call(Some("alice")).await.unwrap();
    assert_eq!(throttled.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(throttled.headers()["retry-after"], "7");
    let body = to_bytes(throttled.into_body(), 4096).await.unwrap();
    let body: SyncErrorBody = serde_json::from_slice(&body).unwrap();
    assert_eq!(body.code, SyncErrorCode::RateLimited);
}
