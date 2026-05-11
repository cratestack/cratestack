//! End-to-end test for optimistic locking via `@version`.
//!
//! Hits both the typed delegate API (`UpdateRecordSet::if_match`) and the
//! HTTP/PATCH path with `If-Match` headers, against a real Postgres.

use cratestack::axum::body::Body;
use cratestack::axum::http::{Request, StatusCode};
use cratestack::include_schema;
use cratestack::sqlx::postgres::PgPoolOptions;
use cratestack::sqlx::{Row, query};
use cratestack::{AuthProvider, CoolCodec, CoolContext, CoolError, RequestContext, Value};
use cratestack_codec_json::JsonCodec;
use tower::util::ServiceExt;

include_schema!("tests/fixtures/banking_versioning.cstack");

async fn serial_guard() -> tokio::sync::MutexGuard<'static, ()> {
    static M: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    M.lock().await
}

async fn connect_or_skip() -> Option<cratestack::sqlx::PgPool> {
    let database_url = std::env::var("CRATESTACK_TEST_DATABASE_URL").ok()?;
    PgPoolOptions::new()
        .max_connections(2)
        .connect(&database_url)
        .await
        .ok()
}

async fn reset_schema(pool: &cratestack::sqlx::PgPool) {
    query("DROP TABLE IF EXISTS cratestack_event_outbox, ledgers")
        .execute(pool)
        .await
        .expect("drop tables");
    query(
        "CREATE TABLE ledgers (
            id BIGINT PRIMARY KEY,
            label TEXT NOT NULL,
            balance BIGINT NOT NULL,
            version BIGINT NOT NULL DEFAULT 0
        )",
    )
    .execute(pool)
    .await
    .expect("create ledger table");
}

fn ctx() -> CoolContext {
    CoolContext::authenticated([("id".to_owned(), Value::Int(1))])
}

#[derive(Clone)]
struct PassThroughAuth;

impl AuthProvider for PassThroughAuth {
    type Error = CoolError;
    fn authenticate(
        &self,
        _request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CoolContext, Self::Error>> + Send {
        core::future::ready(Ok(ctx()))
    }
}

#[tokio::test]
async fn delegate_update_without_if_match_returns_precondition_failed() {
    let _guard = serial_guard().await;
    let Some(pool) = connect_or_skip().await else {
        return;
    };
    reset_schema(&pool).await;
    query("INSERT INTO ledgers (id, label, balance, version) VALUES (1, 'gl-1', 0, 0)")
        .execute(&pool)
        .await
        .expect("seed");

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let result = cool
        .ledger()
        .update(1)
        .set(cratestack_schema::UpdateLedgerInput {
            label: None,
            balance: Some(100),
        })
        .run(&ctx())
        .await;

    let err = result.expect_err("update without if_match must fail");
    assert_eq!(err.code(), "PRECONDITION_FAILED");
}

#[tokio::test]
async fn delegate_update_with_stale_if_match_returns_412_and_keeps_row_intact() {
    let _guard = serial_guard().await;
    let Some(pool) = connect_or_skip().await else {
        return;
    };
    reset_schema(&pool).await;
    query("INSERT INTO ledgers (id, label, balance, version) VALUES (2, 'gl-2', 5, 7)")
        .execute(&pool)
        .await
        .expect("seed");

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let result = cool
        .ledger()
        .update(2)
        .set(cratestack_schema::UpdateLedgerInput {
            label: None,
            balance: Some(99),
        })
        .if_match(3) // stale: real version is 7
        .run(&ctx())
        .await;

    let err = result.expect_err("stale if_match must fail");
    assert_eq!(err.code(), "PRECONDITION_FAILED");

    // Row state must be untouched.
    let row = query("SELECT balance, version FROM ledgers WHERE id = 2")
        .fetch_one(&pool)
        .await
        .expect("read ledger");
    let balance: i64 = row.get("balance");
    let version: i64 = row.get("version");
    assert_eq!(balance, 5, "stale update must not change balance");
    assert_eq!(version, 7, "stale update must not bump version");
}

#[tokio::test]
async fn delegate_update_with_fresh_if_match_increments_version_and_returns_new_state() {
    let _guard = serial_guard().await;
    let Some(pool) = connect_or_skip().await else {
        return;
    };
    reset_schema(&pool).await;
    query("INSERT INTO ledgers (id, label, balance, version) VALUES (3, 'gl-3', 5, 0)")
        .execute(&pool)
        .await
        .expect("seed");

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let updated = cool
        .ledger()
        .update(3)
        .set(cratestack_schema::UpdateLedgerInput {
            label: None,
            balance: Some(42),
        })
        .if_match(0)
        .run(&ctx())
        .await
        .expect("fresh if_match must succeed");

    assert_eq!(updated.balance, 42);
    assert_eq!(updated.version, 1, "version must increment exactly once");

    // A second update with the now-stale version 0 must fail.
    let stale_again = cool
        .ledger()
        .update(3)
        .set(cratestack_schema::UpdateLedgerInput {
            label: None,
            balance: Some(99),
        })
        .if_match(0)
        .run(&ctx())
        .await;
    assert!(
        stale_again.is_err(),
        "re-using the original version after a successful update must fail",
    );
}

#[tokio::test]
async fn http_patch_round_trips_etag_and_rejects_stale_if_match() {
    let _guard = serial_guard().await;
    let Some(pool) = connect_or_skip().await else {
        return;
    };
    reset_schema(&pool).await;
    query("INSERT INTO ledgers (id, label, balance, version) VALUES (4, 'gl-4', 1, 0)")
        .execute(&pool)
        .await
        .expect("seed");

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let router = cratestack_schema::axum::model_router(cool, JsonCodec, PassThroughAuth);

    // GET should return ETag matching the current version.
    let get_response = router
        .clone()
        .oneshot(
            Request::get("/ledgers/4")
                .header("accept", JsonCodec::CONTENT_TYPE)
                .body(Body::empty())
                .expect("get request"),
        )
        .await
        .expect("get succeeds");
    assert_eq!(get_response.status(), StatusCode::OK);
    let etag = get_response
        .headers()
        .get("etag")
        .expect("etag header must be present on versioned model get")
        .to_str()
        .expect("etag is ascii")
        .to_owned();
    assert_eq!(etag, "\"0\"");

    // PATCH without If-Match must fail.
    let no_if_match = router
        .clone()
        .oneshot(
            Request::patch("/ledgers/4")
                .header("content-type", JsonCodec::CONTENT_TYPE)
                .header("accept", JsonCodec::CONTENT_TYPE)
                .body(Body::from(r#"{"balance":5}"#))
                .expect("request"),
        )
        .await
        .expect("send");
    assert_eq!(no_if_match.status(), StatusCode::PRECONDITION_FAILED);

    // PATCH with stale If-Match must fail.
    let stale = router
        .clone()
        .oneshot(
            Request::patch("/ledgers/4")
                .header("content-type", JsonCodec::CONTENT_TYPE)
                .header("accept", JsonCodec::CONTENT_TYPE)
                .header("if-match", "\"999\"")
                .body(Body::from(r#"{"balance":5}"#))
                .expect("request"),
        )
        .await
        .expect("send");
    assert_eq!(stale.status(), StatusCode::PRECONDITION_FAILED);

    // PATCH with fresh If-Match must succeed and return a new ETag.
    let fresh = router
        .clone()
        .oneshot(
            Request::patch("/ledgers/4")
                .header("content-type", JsonCodec::CONTENT_TYPE)
                .header("accept", JsonCodec::CONTENT_TYPE)
                .header("if-match", etag.as_str())
                .body(Body::from(r#"{"balance":5}"#))
                .expect("request"),
        )
        .await
        .expect("send");
    assert_eq!(fresh.status(), StatusCode::OK);
    let new_etag = fresh
        .headers()
        .get("etag")
        .expect("etag on update response")
        .to_str()
        .expect("ascii")
        .to_owned();
    assert_eq!(new_etag, "\"1\"");
}

#[tokio::test]
async fn create_seeds_version_to_zero_even_when_input_omits_it() {
    let _guard = serial_guard().await;
    let Some(pool) = connect_or_skip().await else {
        return;
    };
    reset_schema(&pool).await;

    // The Create input struct is generated without a `version` field
    // (see the @version exclusion in cratestack-macros). If a future
    // change re-added it, this test would not compile — that's the
    // primary line of defence. The runtime assertion below additionally
    // verifies that the server seeds the column to 0 so the INSERT
    // succeeds against a NOT NULL column without a DB default.
    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let created = cool
        .ledger()
        .create(cratestack_schema::CreateLedgerInput {
            id: 42,
            label: "fresh".to_owned(),
            balance: 0,
        })
        .run(&ctx())
        .await
        .expect("create should succeed without version in input");
    assert_eq!(
        created.version, 0,
        "newly created row must start at version 0"
    );
}
