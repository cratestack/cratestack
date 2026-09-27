//! GHSA-r67q-4qqq-g9gm, how an `@isolation` procedure's outcome leaves its
//! transaction (docs/design/procedure-isolation.md §5, §6):
//!
//! - retries exhausted is `409 TRANSACTION_ABORTED` (RPC `aborted`), never
//!   `CONFLICT`, and the idempotency layer does not record it: the same
//!   `Idempotency-Key` runs the body again;
//! - `@computed` output fields are resolved inside the attempt, before
//!   COMMIT, on the attempt's snapshot, and a failing resolver rolls the
//!   attempt back.
//!
//! REST and RPC, from one fixture body. MCP: `procedure_isolation_mcp.rs`.
//! Needs a database: `just test-ci-db --test procedure_isolation_outcome --
//! --test-threads=1` with `CRATESTACK_REQUIRE_DB=1`.

#![cfg(feature = "codec-json")]

mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::extract::ConnectInfo;
use cratestack::axum::http::{Request, StatusCode};
use cratestack::sqlx::{self, PgPool};
use cratestack::{AuthProvider, CratestackContext, CratestackError, RequestContext, Value};
use cratestack_axum::idempotency::IdempotencyLayer;
use cratestack_codec_json::JsonCodec;
use support::pg;
use tower::util::ServiceExt;

#[derive(Default)]
pub struct Shared {
    runs: AtomicUsize,
    resolver_runs: AtomicUsize,
    /// `debitStatement*` waits at `held`/`go` after its debit, before the
    /// output is composed.
    hold: AtomicBool,
    held: tokio::sync::Notify,
    go: tokio::sync::Notify,
    /// The `peerBalance` resolver fails with a validation error.
    fail_resolver: AtomicBool,
    /// The `peerBalance` resolver hits a real 40001 once.
    resolver_conflict_once: AtomicBool,
}

fn db_err(error: sqlx::Error) -> CratestackError {
    cratestack::cratestack_error_from_sqlx(error)
}

const RAISE_40001: &str =
    "DO $$ BEGIN RAISE EXCEPTION 'forced' USING ERRCODE = 'serialization_failure'; END $$";

macro_rules! outcome_impl {
    () => {
        use super::{RAISE_40001, Shared, db_err};
        use cratestack::sqlx;
        use cratestack::{CratestackContext, CratestackError};
        use cratestack_schema::procedures as p;
        use cratestack_schema::{Cratestack, IsolatedCratestack, Report, Statement};
        use std::sync::Arc;
        use std::sync::atomic::Ordering;

        #[derive(Clone)]
        pub struct Procedures(pub Arc<Shared>);

        #[derive(Clone)]
        pub struct Resolvers(pub Arc<Shared>);

        macro_rules! debit_body {
            ($self:ident, $db:ident, $ctx:ident, $args:ident) => {{
                $self.0.runs.fetch_add(1, Ordering::SeqCst);
                let transfer = $args.args;
                let account = $db
                    .out_account()
                    .find_unique(transfer.accountId)
                    .run($ctx)
                    .await?
                    .ok_or_else(|| CratestackError::NotFound("no account".into()))?;
                let updated = $db
                    .out_account()
                    .update(transfer.accountId)
                    .set(cratestack_schema::UpdateOutAccountInput {
                        balance: Some(account.balance - transfer.amount),
                    })
                    .run($ctx)
                    .await?;
                if $self.0.hold.load(Ordering::SeqCst) {
                    $self.0.held.notify_one();
                    $self.0.go.notified().await;
                }
                Ok(Statement {
                    accountId: transfer.accountId,
                    after: updated.balance,
                    peerId: transfer.peerId,
                })
            }};
        }

        impl p::ProcedureRegistry for Procedures {
            async fn always_conflict(
                &self,
                db: &IsolatedCratestack,
                _ctx: &CratestackContext,
                _args: p::always_conflict::Args,
                _authorized: p::always_conflict::Authorized,
            ) -> Result<Report, CratestackError> {
                self.0.runs.fetch_add(1, Ordering::SeqCst);
                db.transaction(async |tx| {
                    sqlx::query(RAISE_40001)
                        .execute(&mut ***tx)
                        .await
                        .map_err(db_err)?;
                    Ok(Report {
                        level: "unreachable".into(),
                    })
                })
                .await
            }

            async fn refuse(
                &self,
                _db: &IsolatedCratestack,
                _ctx: &CratestackContext,
                _args: p::refuse::Args,
                _authorized: p::refuse::Authorized,
            ) -> Result<Report, CratestackError> {
                self.0.runs.fetch_add(1, Ordering::SeqCst);
                Err(CratestackError::Validation("refused".into()))
            }

            async fn debit_statement(
                &self,
                db: &IsolatedCratestack,
                ctx: &CratestackContext,
                args: p::debit_statement::Args,
                _authorized: p::debit_statement::Authorized,
            ) -> Result<Statement, CratestackError> {
                debit_body!(self, db, ctx, args)
            }

            async fn debit_statement_plain(
                &self,
                db: &Cratestack,
                ctx: &CratestackContext,
                args: p::debit_statement_plain::Args,
                _authorized: p::debit_statement_plain::Authorized,
            ) -> Result<Statement, CratestackError> {
                debit_body!(self, db, ctx, args)
            }
        }

        impl cratestack_schema::ComputedFieldResolver for Resolvers {
            fn resolve_statement_peer_balance(
                &self,
                db: &Cratestack,
                source: &Statement,
                ctx: &CratestackContext,
            ) -> impl core::future::Future<Output = Result<i64, CratestackError>> + Send {
                let (db, peer, ctx, shared) =
                    (db.clone(), source.peerId, ctx.clone(), self.0.clone());
                async move {
                    shared.resolver_runs.fetch_add(1, Ordering::SeqCst);
                    if shared.fail_resolver.load(Ordering::SeqCst) {
                        return Err(CratestackError::Validation("resolver refused".into()));
                    }
                    if shared.resolver_conflict_once.swap(false, Ordering::SeqCst) {
                        db.transaction(async |tx| {
                            sqlx::query(RAISE_40001)
                                .execute(&mut ***tx)
                                .await
                                .map(|_| ())
                                .map_err(db_err)
                        })
                        .await?;
                    }
                    let account = db
                        .out_account()
                        .find_unique(peer)
                        .run(&ctx)
                        .await?
                        .ok_or_else(|| CratestackError::NotFound("no peer".into()))?;
                    Ok(account.balance)
                }
            }
        }
    };
}

pub mod rest {
    use cratestack::include_server_schema;
    include_server_schema!(
        "tests/fixtures/procedure_isolation_outcome.cstack",
        db = Postgres
    );
    outcome_impl!();
}

pub mod rpc {
    use cratestack::include_server_schema;
    include_server_schema!(
        "tests/fixtures/procedure_isolation_outcome_rpc.cstack",
        db = Postgres
    );
    outcome_impl!();
}

#[derive(Clone)]
struct AlwaysAuth;

impl AuthProvider for AlwaysAuth {
    type Error = CratestackError;

    fn authenticate(
        &self,
        _request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        core::future::ready(Ok(CratestackContext::authenticated([(
            "id".to_owned(),
            Value::Int(1),
        )])))
    }
}

const TRANSPORTS: [(&str, bool); 2] = [("REST", false), ("RPC", true)];

macro_rules! mount {
    ($module:ident, $mount:ident, $pool:expr, $shared:expr) => {{
        let db = $module::cratestack_schema::Cratestack::builder($pool.clone())
            .with_isolation_max_retries(0)
            .build();
        $module::cratestack_schema::axum::$mount(
            db,
            $module::Procedures($shared.clone()),
            $module::Resolvers($shared.clone()),
            JsonCodec,
            AlwaysAuth,
            cratestack::DEFAULT_BODY_LIMIT_BYTES,
        )
    }};
}

/// A router with retry budget 0, behind an `IdempotencyLayer` over the
/// real Postgres store.
fn router(pool: &PgPool, rpc: bool) -> (cratestack::axum::Router, Arc<Shared>) {
    let shared = Arc::new(Shared::default());
    let router = if rpc {
        mount!(rpc, rpc_router, pool, shared)
    } else {
        mount!(rest, router, pool, shared)
    };
    let store = Arc::new(cratestack::SqlxIdempotencyStore::new(pool.clone()));
    let router = router.layer(IdempotencyLayer::new(store, Duration::from_secs(60)));
    (router, shared)
}

fn uri(rpc: bool, procedure: &str) -> String {
    if rpc {
        format!("/rpc/procedure.{procedure}")
    } else {
        format!("/$procs/{procedure}")
    }
}

async fn post(
    router: &cratestack::axum::Router,
    uri: &str,
    key: Option<&str>,
    body: &str,
) -> (StatusCode, String) {
    let mut request = Request::post(uri)
        .header("content-type", "application/json")
        .header("accept", "application/json");
    if let Some(key) = key {
        request = request.header("idempotency-key", key);
    }
    let mut request = request.body(Body::from(body.to_owned())).unwrap();
    let peer: std::net::SocketAddr = "192.0.2.93:1".parse().unwrap();
    request.extensions_mut().insert(ConnectInfo(peer));
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

async fn reset(pool: &PgPool) {
    cratestack::SqlxIdempotencyStore::new(pool.clone())
        .ensure_schema()
        .await
        .expect("idempotency table");
    for statement in [
        "DELETE FROM cratestack_idempotency",
        "DROP TABLE IF EXISTS out_accounts",
        "CREATE TABLE out_accounts (id BIGINT PRIMARY KEY, balance BIGINT NOT NULL)",
        "INSERT INTO out_accounts VALUES (1, 100), (2, 500)",
    ] {
        sqlx::query(sqlx::AssertSqlSafe(statement.to_owned()))
            .execute(pool)
            .await
            .unwrap_or_else(|error| panic!("{statement}: {error}"));
    }
}

async fn balance(pool: &PgPool, id: i64) -> i64 {
    sqlx::query_scalar("SELECT balance FROM out_accounts WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

const PROBE: &str = r#"{"args":{"nonce":"x"}}"#;

/// Retries exhausted is 409 with its own code, on both transports and in a
/// `/rpc/batch` frame — never the `CONFLICT` a unique violation carries.
#[tokio::test]
async fn exhausted_retries_carry_their_own_code() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool.clone();
    reset(&pool).await;
    for (label, rpc) in TRANSPORTS {
        let (router, shared) = router(&pool, rpc);
        let (status, text) = post(&router, &uri(rpc, "alwaysConflict"), None, PROBE).await;
        println!("{label} alwaysConflict: {status} {text}");
        assert_eq!(status, StatusCode::CONFLICT, "{label}: {text}");
        let code = if rpc {
            "aborted"
        } else {
            "TRANSACTION_ABORTED"
        };
        assert!(
            text.contains(&format!(r#""code":"{code}""#)),
            "{label}: {text}"
        );
        assert!(
            !text.to_lowercase().contains(r#""code":"conflict""#),
            "{label}: {text}"
        );
        assert!(
            text.contains("concurrent updates; retry the request"),
            "{label}: {text}"
        );
        assert_eq!(shared.runs.load(Ordering::SeqCst), 1, "retry budget 0");
    }

    let (router, _) = router(&pool, true);
    let batch = r#"[{"id":1,"op":"procedure.alwaysConflict","input":{"args":{"nonce":"x"}}}]"#;
    let (status, text) = post(&router, "/rpc/batch", None, batch).await;
    println!("RPC batch alwaysConflict: {status} {text}");
    assert_eq!(status, StatusCode::OK, "{text}");
    assert!(text.contains(r#""code":"aborted""#), "{text}");
}

/// The same `Idempotency-Key` after an aborted call runs the body again: it
/// is not a replay of the 409. Any other error under a key is still recorded
/// and replayed — the proof that the layer is live on this router.
#[tokio::test]
async fn an_aborted_call_is_not_recorded_under_its_key() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool.clone();
    for (label, rpc) in TRANSPORTS {
        reset(&pool).await;
        let (router, shared) = router(&pool, rpc);
        let aborted = uri(rpc, "alwaysConflict");
        let first = post(&router, &aborted, Some("k-aborted"), PROBE).await;
        let second = post(&router, &aborted, Some("k-aborted"), PROBE).await;
        println!("{label} aborted x2: {first:?} {second:?}");
        assert_eq!(first.0, StatusCode::CONFLICT, "{label}: {first:?}");
        assert_eq!(second.0, StatusCode::CONFLICT, "{label}: {second:?}");
        assert_eq!(
            shared.runs.load(Ordering::SeqCst),
            2,
            "{label}: the same key ran the body again"
        );

        let refused = uri(rpc, "refuse");
        let first = post(&router, &refused, Some("k-refused"), PROBE).await;
        let second = post(&router, &refused, Some("k-refused"), PROBE).await;
        println!("{label} refused x2: {first:?} {second:?}");
        assert_eq!(
            first, second,
            "{label}: the replay is the recorded response"
        );
        assert_eq!(
            shared.runs.load(Ordering::SeqCst),
            3,
            "{label}: a validation error is recorded and replayed, not rerun"
        );
    }
}

/// A resolver that reads a row a concurrent session changes after the body
/// ran: inside the attempt it sees the attempt's snapshot; without
/// `@isolation` (composed after commit, on the pool) it sees the change.
#[tokio::test]
async fn computed_fields_read_the_attempts_snapshot() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool.clone();
    for (label, rpc) in TRANSPORTS {
        for (procedure, expected_peer) in [("debitStatement", 500), ("debitStatementPlain", 7)] {
            reset(&pool).await;
            let (router, shared) = router(&pool, rpc);
            shared.hold.store(true, Ordering::SeqCst);
            let call = {
                let router = router.clone();
                let uri = uri(rpc, procedure);
                tokio::spawn(async move {
                    post(
                        &router,
                        &uri,
                        None,
                        r#"{"args":{"accountId":1,"peerId":2,"amount":10}}"#,
                    )
                    .await
                })
            };
            tokio::time::timeout(Duration::from_secs(10), shared.held.notified())
                .await
                .expect("the body reached its hold point");
            sqlx::query("UPDATE out_accounts SET balance = 7 WHERE id = 2")
                .execute(&pool)
                .await
                .unwrap();
            shared.go.notify_one();
            let (status, text) = call.await.unwrap();
            println!("{label} {procedure}: {status} {text}");
            assert_eq!(status, StatusCode::OK, "{label} {procedure}: {text}");
            let body: serde_json::Value = serde_json::from_str(&text).unwrap();
            assert_eq!(body["after"], 90, "{label} {procedure}: {text}");
            assert_eq!(
                body["peerBalance"], expected_peer,
                "{label} {procedure}: {text}"
            );
            assert_eq!(balance(&pool, 1).await, 90);
        }
    }
}

/// A resolver that fails fails the attempt: the body's debit is rolled back,
/// the caller gets the resolver's error, and it is not retried. Without
/// `@isolation` the same failure arrives after the debit committed.
#[tokio::test]
async fn a_failing_resolver_rolls_the_attempt_back() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool.clone();
    let body = r#"{"args":{"accountId":1,"peerId":2,"amount":10}}"#;
    for (label, rpc) in TRANSPORTS {
        for (procedure, committed) in [("debitStatement", false), ("debitStatementPlain", true)] {
            reset(&pool).await;
            let (router, shared) = router(&pool, rpc);
            shared.fail_resolver.store(true, Ordering::SeqCst);
            let (status, text) = post(&router, &uri(rpc, procedure), None, body).await;
            println!("{label} {procedure} failing resolver: {status} {text}");
            assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{label}: {text}");
            assert!(text.contains("resolver refused"), "{label}: {text}");
            assert_eq!(
                shared.runs.load(Ordering::SeqCst),
                1,
                "{label}: not retried"
            );
            let expected = if committed { 90 } else { 100 };
            assert_eq!(
                balance(&pool, 1).await,
                expected,
                "{label} {procedure}: debit committed = {committed}"
            );
        }
    }
}

/// A serialization failure a resolver hits is retriable like one from the
/// body: the attempt is rolled back and run again, resolver included.
#[tokio::test]
async fn a_resolver_serialization_failure_retries_the_attempt() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool.clone();
    reset(&pool).await;
    let shared = Arc::new(Shared::default());
    let db = rest::cratestack_schema::Cratestack::builder(pool.clone()).build();
    let router = rest::cratestack_schema::axum::router(
        db,
        rest::Procedures(shared.clone()),
        rest::Resolvers(shared.clone()),
        JsonCodec,
        AlwaysAuth,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    );
    shared.resolver_conflict_once.store(true, Ordering::SeqCst);
    let (status, text) = post(
        &router,
        "/$procs/debitStatement",
        None,
        r#"{"args":{"accountId":1,"peerId":2,"amount":10}}"#,
    )
    .await;
    println!("resolver 40001 once: {status} {text}");
    assert_eq!(status, StatusCode::OK, "{text}");
    assert_eq!(
        shared.runs.load(Ordering::SeqCst),
        2,
        "the attempt ran again"
    );
    assert_eq!(shared.resolver_runs.load(Ordering::SeqCst), 2);
    assert_eq!(balance(&pool, 1).await, 90, "debited once");
}
