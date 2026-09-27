//! GHSA-r67q-4qqq-g9gm, adversarial pass: can an `@isolation` procedure's
//! work leave its transaction, be committed only in part, be silently
//! dropped while the caller is told it succeeded, or wedge a small pool?
//! docs/design/procedure-isolation.md. The first-pass suite is
//! `procedure_isolation.rs`.
//!
//! Needs a database: `just test-ci-db --test procedure_isolation_escape --
//! --test-threads=1` with `CRATESTACK_REQUIRE_DB=1`. A skip prints `ok` in
//! 0.00s.

#![cfg(feature = "codec-json")]

mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::extract::ConnectInfo;
use cratestack::axum::http::{Request, StatusCode};
use cratestack::sqlx::{self, PgPool, postgres::PgPoolOptions};
use cratestack::{
    AuthProvider, CratestackContext, CratestackError, RequestContext, Value, include_server_schema,
};
use cratestack_codec_json::JsonCodec;
use support::pg;
use tower::util::ServiceExt;

include_server_schema!(
    "tests/fixtures/procedure_isolation_escape.cstack",
    db = Postgres
);

use cratestack_schema::procedures as p;
use cratestack_schema::{
    Cratestack, CreateEscAccountInput, IsolatedCratestack, Receipt, Report, UpdateEscAccountInput,
};

fn db_err(error: sqlx::Error) -> CratestackError {
    cratestack::cratestack_error_from_sqlx(error)
}

struct Shared {
    runs: AtomicUsize,
    arrived: AtomicUsize,
    parties: usize,
    delivered: Arc<AtomicUsize>,
    stall: tokio::sync::Notify,
    held: tokio::sync::Notify,
    go: tokio::sync::Notify,
}

impl Shared {
    async fn gate(&self) {
        self.arrived.fetch_add(1, Ordering::SeqCst);
        let deadline = Instant::now() + Duration::from_secs(5);
        while self.arrived.load(Ordering::SeqCst) < self.parties && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
}

#[derive(Clone)]
struct Procedures(Arc<Shared>);

async fn debit(
    db: &IsolatedCratestack,
    ctx: &CratestackContext,
    id: i64,
    amount: i64,
) -> Result<Receipt, CratestackError> {
    let account = db
        .esc_account()
        .find_unique(id)
        .run(ctx)
        .await?
        .ok_or_else(|| CratestackError::NotFound("no account".into()))?;
    let updated = db
        .esc_account()
        .update(id)
        .set(UpdateEscAccountInput {
            balance: Some(account.balance - amount),
        })
        .run(ctx)
        .await?;
    Ok(Receipt {
        before: account.balance,
        after: updated.balance,
    })
}

impl p::ProcedureRegistry for Procedures {
    async fn swallow_broken_nested(
        &self,
        db: &IsolatedCratestack,
        ctx: &CratestackContext,
        args: p::swallow_broken_nested::Args,
        _authorized: p::swallow_broken_nested::Authorized,
    ) -> Result<Receipt, CratestackError> {
        self.0.runs.fetch_add(1, Ordering::SeqCst);
        let receipt = debit(db, ctx, args.args.accountId, args.args.amount).await?;
        // "Best effort" bookkeeping written carelessly: the statement fails,
        // the closure ignores it, and the caller ignores what
        // `transaction` then returns.
        let _ = db
            .transaction(async |tx| {
                let _ = sqlx::query("SELECT 1 / 0").execute(&mut ***tx).await;
                Ok(())
            })
            .await;
        Ok(receipt)
    }

    async fn raw_commit(
        &self,
        db: &IsolatedCratestack,
        ctx: &CratestackContext,
        args: p::raw_commit::Args,
        _authorized: p::raw_commit::Authorized,
    ) -> Result<Receipt, CratestackError> {
        self.0.runs.fetch_add(1, Ordering::SeqCst);
        let first = debit(db, ctx, args.args.accountId, args.args.amount).await?;
        db.transaction(async |tx| {
            sqlx::query("COMMIT")
                .execute(&mut ***tx)
                .await
                .map_err(db_err)?;
            Ok(())
        })
        .await?;
        let second = debit(db, ctx, args.args.accountId, args.args.amount).await?;
        Ok(Receipt {
            before: first.before,
            after: second.after,
        })
    }

    async fn downgrade(
        &self,
        db: &IsolatedCratestack,
        _ctx: &CratestackContext,
        _args: p::downgrade::Args,
        _authorized: p::downgrade::Authorized,
    ) -> Result<Report, CratestackError> {
        let level = db
            .transaction(async |tx| {
                sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED")
                    .execute(&mut ***tx)
                    .await
                    .map_err(db_err)?;
                sqlx::query_scalar::<_, String>("SELECT current_setting('transaction_isolation')")
                    .fetch_one(&mut ***tx)
                    .await
                    .map_err(db_err)
            })
            .await?;
        Ok(Report { level })
    }

    async fn transfer_rc(
        &self,
        db: &IsolatedCratestack,
        ctx: &CratestackContext,
        args: p::transfer_rc::Args,
        _authorized: p::transfer_rc::Authorized,
    ) -> Result<Receipt, CratestackError> {
        self.0.runs.fetch_add(1, Ordering::SeqCst);
        let from = debit(db, ctx, args.args.fromId, args.args.amount).await?;
        // Both callers hold their first row's lock before either takes the
        // second: opposite transfers deadlock, and Postgres kills one.
        self.0.gate().await;
        debit(db, ctx, args.args.toId, -args.args.amount).await?;
        Ok(from)
    }

    async fn everything(
        &self,
        db: &IsolatedCratestack,
        ctx: &CratestackContext,
        args: p::everything::Args,
        _authorized: p::everything::Authorized,
    ) -> Result<Receipt, CratestackError> {
        self.0.runs.fetch_add(1, Ordering::SeqCst);
        let id = args.args.accountId;
        let _all = db.esc_account().find_many().run(ctx).await?;
        let _count = db.esc_account().aggregate().count().run(ctx).await?;
        let receipt = debit(db, ctx, id, args.args.amount).await?;
        db.esc_account()
            .create(CreateEscAccountInput {
                id: id + 100,
                balance: 1,
            })
            .run(ctx)
            .await?;
        db.esc_account()
            .batch_create(vec![
                CreateEscAccountInput {
                    id: id + 200,
                    balance: 1,
                },
                CreateEscAccountInput {
                    id: id + 201,
                    balance: 1,
                },
            ])
            .run(ctx)
            .await?;
        db.esc_account().delete(id + 100).run(ctx).await?;
        let composed = db
            .transaction(async |tx| {
                db.esc_account()
                    .create(CreateEscAccountInput {
                        id: id + 300,
                        balance: 1,
                    })
                    .run_in_tx(tx, ctx)
                    .await
            })
            .await?;
        db.dispatch_audit_sink(&composed.audit_events).await;
        let bound = db.bind_context(ctx.clone());
        bound
            .esc_account()
            .find_unique(id)
            .run()
            .await?
            .ok_or_else(|| CratestackError::NotFound("gone".into()))?;
        Ok(receipt)
    }

    async fn withdraw_nested_rr(
        &self,
        db: &IsolatedCratestack,
        ctx: &CratestackContext,
        args: p::withdraw_nested_rr::Args,
        _authorized: p::withdraw_nested_rr::Authorized,
    ) -> Result<Receipt, CratestackError> {
        self.0.runs.fetch_add(1, Ordering::SeqCst);
        let id = args.args.accountId;
        let amount = args.args.amount;
        let account = db
            .esc_account()
            .find_unique(id)
            .run(ctx)
            .await?
            .ok_or_else(|| CratestackError::NotFound("no account".into()))?;
        self.0.gate().await;
        if account.balance < amount {
            return Err(CratestackError::Validation("insufficient funds".into()));
        }
        // The closure returns the failed debit; the body ignores it.
        let after = db
            .transaction(async |tx| {
                db.esc_account()
                    .update(id)
                    .set(UpdateEscAccountInput {
                        balance: Some(account.balance - amount),
                    })
                    .run_in_tx(tx, ctx)
                    .await
            })
            .await
            .map(|outcome| outcome.value.balance)
            .unwrap_or(account.balance);
        Ok(Receipt {
            before: account.balance,
            after,
        })
    }

    async fn stall(
        &self,
        db: &IsolatedCratestack,
        ctx: &CratestackContext,
        args: p::stall::Args,
        _authorized: p::stall::Authorized,
    ) -> Result<Receipt, CratestackError> {
        self.0.runs.fetch_add(1, Ordering::SeqCst);
        let receipt = debit(db, ctx, args.args.accountId, args.args.amount).await?;
        self.0.stall.notified().await;
        Ok(receipt)
    }

    async fn refuse_loudly(
        &self,
        _db: &IsolatedCratestack,
        _ctx: &CratestackContext,
        args: p::refuse_loudly::Args,
        _authorized: p::refuse_loudly::Authorized,
    ) -> Result<Receipt, CratestackError> {
        self.0.runs.fetch_add(1, Ordering::SeqCst);
        Err(CratestackError::Validation(format!(
            "insufficient funds: requested {}",
            args.args.amount
        )))
    }

    async fn always_conflict(
        &self,
        db: &IsolatedCratestack,
        _ctx: &CratestackContext,
        _args: p::always_conflict::Args,
        _authorized: p::always_conflict::Authorized,
    ) -> Result<Report, CratestackError> {
        self.0.runs.fetch_add(1, Ordering::SeqCst);
        db.transaction(async |tx| {
            sqlx::query(
                "DO $$ BEGIN RAISE EXCEPTION 'forced' USING ERRCODE = 'serialization_failure'; \
                 END $$",
            )
            .execute(&mut ***tx)
            .await
            .map_err(db_err)?;
            Ok(Report {
                level: "unreachable".into(),
            })
        })
        .await
    }

    async fn commit_time_conflict(
        &self,
        db: &IsolatedCratestack,
        ctx: &CratestackContext,
        args: p::commit_time_conflict::Args,
        _authorized: p::commit_time_conflict::Authorized,
    ) -> Result<Receipt, CratestackError> {
        let run = self.0.runs.fetch_add(1, Ordering::SeqCst);
        db.esc_account()
            .find_unique(args.args.fromId)
            .run(ctx)
            .await?;
        let receipt = debit(db, ctx, args.args.toId, args.args.amount).await?;
        if run == 0 {
            self.0.held.notify_one();
            self.0.go.notified().await;
        }
        Ok(receipt)
    }

    async fn raw_rollback_swallowed(
        &self,
        db: &IsolatedCratestack,
        ctx: &CratestackContext,
        args: p::raw_rollback_swallowed::Args,
        _authorized: p::raw_rollback_swallowed::Authorized,
    ) -> Result<Receipt, CratestackError> {
        self.0.runs.fetch_add(1, Ordering::SeqCst);
        let receipt = debit(db, ctx, args.args.accountId, args.args.amount).await?;
        let _ = db
            .transaction(async |tx| {
                sqlx::query("ROLLBACK")
                    .execute(&mut ***tx)
                    .await
                    .map_err(db_err)?;
                Err::<(), _>(CratestackError::Validation("changed my mind".into()))
            })
            .await;
        Ok(receipt)
    }
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

fn router(pool: &PgPool, parties: usize) -> (cratestack::axum::Router, Arc<Shared>) {
    router_with_retries(pool, parties, None)
}

fn router_with_retries(
    pool: &PgPool,
    parties: usize,
    retries: Option<u32>,
) -> (cratestack::axum::Router, Arc<Shared>) {
    let shared = Arc::new(Shared {
        runs: AtomicUsize::new(0),
        arrived: AtomicUsize::new(0),
        parties,
        delivered: Arc::new(AtomicUsize::new(0)),
        stall: tokio::sync::Notify::new(),
        held: tokio::sync::Notify::new(),
        go: tokio::sync::Notify::new(),
    });
    let mut builder = Cratestack::builder(pool.clone());
    if let Some(retries) = retries {
        builder = builder.with_isolation_max_retries(retries);
    }
    let db = builder.build();
    let delivered = shared.delivered.clone();
    db.events().on_esc_account_updated(move |_event| {
        let delivered = delivered.clone();
        async move {
            delivered.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    });
    let router = cratestack_schema::axum::router(
        db,
        Procedures(shared.clone()),
        (),
        JsonCodec,
        AlwaysAuth,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    );
    (router, shared)
}

async fn post(
    router: cratestack::axum::Router,
    procedure: &str,
    body: &str,
) -> (StatusCode, String) {
    let mut request = Request::post(format!("/$procs/{procedure}").as_str())
        .header("content-type", "application/json")
        .header("accept", "application/json")
        .body(Body::from(body.to_owned()))
        .unwrap();
    let peer: std::net::SocketAddr = "192.0.2.92:1".parse().unwrap();
    request.extensions_mut().insert(ConnectInfo(peer));
    let response = router.oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

async fn reset(pool: &PgPool, accounts: &[(i64, i64)]) {
    sqlx::raw_sql(cratestack::AUDIT_TABLE_DDL)
        .execute(pool)
        .await
        .unwrap();
    Cratestack::builder(pool.clone())
        .build()
        .events()
        .drain()
        .await
        .unwrap();
    for statement in [
        "DELETE FROM cratestack_event_outbox WHERE model = 'EscAccount'",
        "DROP TABLE IF EXISTS esc_accounts",
        "CREATE TABLE esc_accounts (id BIGINT PRIMARY KEY, balance BIGINT NOT NULL)",
        "DELETE FROM cratestack_audit WHERE model = 'EscAccount'",
    ] {
        sqlx::query(statement).execute(pool).await.unwrap();
    }
    for (id, balance) in accounts {
        sqlx::query("INSERT INTO esc_accounts (id, balance) VALUES ($1, $2)")
            .bind(id)
            .bind(balance)
            .execute(pool)
            .await
            .unwrap();
    }
}

async fn balances(pool: &PgPool) -> Vec<(i64, i64)> {
    sqlx::query_as("SELECT id, balance FROM esc_accounts ORDER BY id")
        .fetch_all(pool)
        .await
        .unwrap()
}

async fn one_connection_pool(url: &str) -> PgPool {
    PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(3))
        .connect(url)
        .await
        .unwrap()
}

/// A 200 means the debit happened. Before this was fixed the nested
/// `transaction` left the isolated transaction aborted, `COMMIT` on an
/// aborted transaction is a silent `ROLLBACK` in Postgres, and the caller
/// was told 100 left an account that still held it (measured: `200
/// {"before":100,"after":0}`, balance 100).
#[tokio::test]
async fn a_success_response_means_the_work_was_committed() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool.clone();
    reset(&pool, &[(1, 100)]).await;
    let (router, _) = router(&pool, 1);
    let (status, text) = post(
        router,
        "swallowBrokenNested",
        r#"{"args":{"accountId":1,"amount":100}}"#,
    )
    .await;
    let after = balances(&pool).await;
    println!("swallowBrokenNested: {status} {text} balances={after:?}");
    if status == StatusCode::OK {
        assert_eq!(after, vec![(1, 0)], "200 but the debit is gone: {text}");
    }
    // Fail closed: the attempt whose nested savepoint could not be closed
    // is rolled back as a whole and reported as the error it is.
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{text}");
    assert_eq!(after, vec![(1, 100)], "an error, and nothing committed");
}

/// Raw `COMMIT` through the nested `tx` ends the isolated transaction.
/// Whatever the response, the caller must not be told the whole procedure
/// succeeded when only part of it was isolated.
#[tokio::test]
async fn a_raw_commit_through_tx_is_not_a_success() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool.clone();
    reset(&pool, &[(1, 100)]).await;
    let (router, _) = router(&pool, 1);
    let (status, text) = post(
        router,
        "rawCommit",
        r#"{"args":{"accountId":1,"amount":10}}"#,
    )
    .await;
    let after = balances(&pool).await;
    println!("rawCommit: {status} {text} balances={after:?}");
    assert_ne!(status, StatusCode::OK, "{text} {after:?}");
}

/// Postgres refuses `SET TRANSACTION` inside a savepoint, and the nested
/// `tx` is always inside one: the declared level cannot be lowered.
#[tokio::test]
async fn the_level_cannot_be_lowered_through_tx() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool.clone();
    reset(&pool, &[]).await;
    let (router, _) = router(&pool, 1);
    let (status, text) = post(router, "downgrade", r#"{"args":{"nonce":"x"}}"#).await;
    println!("downgrade: {status} {text}");
    assert_ne!(status, StatusCode::OK, "{text}");
    assert!(!text.contains("read committed"), "{text}");
}

/// Explicit `read_committed` is still one transaction, and a deadlock
/// (40P01) is retried like a serialization failure.
#[tokio::test]
async fn opposite_transfers_deadlock_and_the_victim_is_retried() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&test_pg.url)
        .await
        .unwrap();
    reset(&pool, &[(1, 100), (2, 100)]).await;
    let (router, shared) = router(&pool, 2);
    let (a, b) = tokio::join!(
        post(
            router.clone(),
            "transferRc",
            r#"{"args":{"fromId":1,"toId":2,"amount":30}}"#
        ),
        post(
            router.clone(),
            "transferRc",
            r#"{"args":{"fromId":2,"toId":1,"amount":50}}"#
        ),
    );
    let after = balances(&pool).await;
    let runs = shared.runs.load(Ordering::SeqCst);
    println!("transferRc A={a:?} B={b:?} balances={after:?} runs={runs}");
    assert_eq!((a.0, b.0), (StatusCode::OK, StatusCode::OK), "{a:?} {b:?}");
    assert_eq!(after, vec![(1, 120), (2, 80)]);
    assert!(runs >= 3, "the deadlock victim ran again: {runs}");
}

/// Every door on the handle, on a one-connection pool: nothing may need a
/// second connection while the isolated transaction holds the only one.
/// With an escape to the pool this waits out `acquire_timeout` and fails.
#[tokio::test]
async fn every_door_works_on_a_one_connection_pool() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = one_connection_pool(&test_pg.url).await;
    reset(&pool, &[(1, 100)]).await;
    let (router, shared) = router(&pool, 1);
    let started = Instant::now();
    let (status, text) = post(
        router.clone(),
        "everything",
        r#"{"args":{"accountId":1,"amount":10}}"#,
    )
    .await;
    let elapsed = started.elapsed();
    let after = balances(&pool).await;
    println!("everything: {status} {text} in {elapsed:?} balances={after:?}");
    assert_eq!(status, StatusCode::OK, "{text}");
    assert!(
        elapsed < Duration::from_secs(3),
        "waited for a second connection: {elapsed:?}"
    );
    assert_eq!(after, vec![(1, 90), (201, 1), (202, 1), (301, 1)]);
    assert_eq!(
        shared.delivered.load(Ordering::SeqCst),
        1,
        "one update event"
    );

    // Several at once on the same single connection: they queue, none hangs.
    let started = Instant::now();
    let (x, y, z) = tokio::join!(
        post(
            router.clone(),
            "transferRc",
            r#"{"args":{"fromId":1,"toId":201,"amount":1}}"#
        ),
        post(
            router.clone(),
            "everything",
            r#"{"args":{"accountId":2,"amount":0}}"#
        ),
        post(
            router.clone(),
            "transferRc",
            r#"{"args":{"fromId":202,"toId":1,"amount":1}}"#
        ),
    );
    println!("queued: {x:?} {y:?} {z:?} in {:?}", started.elapsed());
    assert_eq!((x.0, z.0), (StatusCode::OK, StatusCode::OK), "{x:?} {z:?}");
    // No account 2: `@authorize` refuses it inside the transaction.
    assert!(y.0.is_client_error(), "{y:?}");
}

/// The caller goes away mid-body (client disconnect, timeout). The attempt
/// is rolled back and the pool's only connection comes back usable.
#[tokio::test]
async fn a_cancelled_call_rolls_back_and_frees_its_connection() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = one_connection_pool(&test_pg.url).await;
    reset(&pool, &[(1, 100), (2, 100)]).await;
    let (router, _) = router(&pool, 1);
    let cancelled = tokio::time::timeout(
        Duration::from_millis(500),
        post(
            router.clone(),
            "stall",
            r#"{"args":{"accountId":1,"amount":100}}"#,
        ),
    )
    .await;
    assert!(cancelled.is_err(), "the stalled call was cut off");
    let (status, text) = post(
        router.clone(),
        "transferRc",
        r#"{"args":{"fromId":2,"toId":1,"amount":5}}"#,
    )
    .await;
    let after = balances(&pool).await;
    println!("after cancel: {status} {text} balances={after:?}");
    assert_eq!(status, StatusCode::OK, "{text}");
    assert_eq!(after, vec![(1, 105), (2, 95)], "the stalled debit is gone");
    let idle_in_tx: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::BIGINT FROM pg_stat_activity \
         WHERE state LIKE 'idle in transaction%' AND datname = current_database()",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(idle_in_tx, 0, "no transaction left open");
}

/// Repeatable read: the loser's debit, made inside a nested `transaction`,
/// fails with 40001 ("concurrent update"); the closure returns it, the body
/// ignores it. Postgres would commit the rest of that attempt, so only the
/// taint makes it retry — and the retry reads 0 and refuses.
#[tokio::test]
async fn a_serialization_failure_returned_by_a_nested_transaction_retries() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool.clone();
    reset(&pool, &[(1, 100)]).await;
    let (router, shared) = router(&pool, 2);
    let body = r#"{"args":{"accountId":1,"amount":100}}"#;
    let (a, b) = tokio::join!(
        post(router.clone(), "withdrawNestedRr", body),
        post(router.clone(), "withdrawNestedRr", body),
    );
    let after = balances(&pool).await;
    let runs = shared.runs.load(Ordering::SeqCst);
    println!("withdrawNestedRr A={a:?} B={b:?} balances={after:?} runs={runs}");
    let oks: Vec<_> = [&a, &b]
        .into_iter()
        .filter(|r| r.0 == StatusCode::OK)
        .collect();
    assert_eq!(oks.len(), 1, "{a:?} {b:?}");
    assert!(
        oks[0].1.contains(r#""after":0"#),
        "the winner debited: {oks:?}"
    );
    let refused = if a.0 == StatusCode::OK { &b } else { &a };
    assert!(refused.1.contains("insufficient funds"), "{refused:?}");
    assert!(runs >= 3, "the loser ran again: {runs}");
    assert_eq!(after, vec![(1, 0)]);
}

/// An application error is not a serialization failure because its text
/// contains "40001". Before the fix the body ran 4 times and the caller got
/// 409 CONFLICT instead of the 422.
#[tokio::test]
async fn an_application_error_mentioning_40001_is_not_retried() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool.clone();
    reset(&pool, &[]).await;
    let (router, shared) = router(&pool, 1);
    let (status, text) = post(
        router,
        "refuseLoudly",
        r#"{"args":{"accountId":1,"amount":40001}}"#,
    )
    .await;
    let runs = shared.runs.load(Ordering::SeqCst);
    println!("refuseLoudly: {status} {text} runs={runs}");
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{text}");
    assert_eq!(runs, 1, "not retried");
}

/// The retry budget is exact: `n` retries is `n + 1` attempts, then 409.
#[tokio::test]
async fn the_retry_budget_is_exact() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool.clone();
    reset(&pool, &[]).await;
    for (retries, attempts) in [(None, 4), (Some(0), 1), (Some(1), 2)] {
        let (router, shared) = router_with_retries(&pool, 1, retries);
        let (status, text) = post(router, "alwaysConflict", r#"{"args":{"nonce":"x"}}"#).await;
        let runs = shared.runs.load(Ordering::SeqCst);
        println!("alwaysConflict retries={retries:?}: {status} {text} runs={runs}");
        assert_eq!(status, StatusCode::CONFLICT, "{text}");
        assert!(text.contains(r#""code":"TRANSACTION_ABORTED""#), "{text}");
        assert_eq!(runs, attempts, "retries={retries:?}");
    }
}

/// A serialization failure Postgres raises at `COMMIT` ("Canceled on
/// identification as a pivot, during commit attempt") is retried, not
/// returned. The first attempt reads 1 and writes 2, then waits while a
/// second transaction reads 2, writes 1 and commits first.
#[tokio::test]
async fn a_serialization_failure_at_commit_is_retried() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool.clone();
    reset(&pool, &[(1, 100), (2, 100)]).await;
    let (router, shared) = router(&pool, 1);
    let call = tokio::spawn(post(
        router,
        "commitTimeConflict",
        r#"{"args":{"fromId":1,"toId":2,"amount":10}}"#,
    ));
    tokio::time::timeout(Duration::from_secs(10), shared.held.notified())
        .await
        .expect("the first attempt reached its hold point");
    let mut other = pool
        .begin_with("BEGIN ISOLATION LEVEL SERIALIZABLE")
        .await
        .unwrap();
    sqlx::query("SELECT balance FROM esc_accounts WHERE id = 2")
        .fetch_one(&mut *other)
        .await
        .unwrap();
    sqlx::query("UPDATE esc_accounts SET balance = balance - 1 WHERE id = 1")
        .execute(&mut *other)
        .await
        .unwrap();
    other.commit().await.unwrap();
    shared.go.notify_one();
    let (status, text) = call.await.unwrap();
    let after = balances(&pool).await;
    let runs = shared.runs.load(Ordering::SeqCst);
    println!("commitTimeConflict: {status} {text} runs={runs} balances={after:?}");
    assert_eq!(status, StatusCode::OK, "{text}");
    assert_eq!(runs, 2, "the attempt whose COMMIT failed ran again");
    assert_eq!(after, vec![(1, 99), (2, 90)]);
}

/// A raw `ROLLBACK` in a nested `transaction` throws away the debit made
/// before it; the closure's error is ignored by the body. The savepoint
/// cannot be rolled back to, so the attempt must not report success.
#[tokio::test]
async fn a_raw_rollback_through_tx_is_not_a_success() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool.clone();
    reset(&pool, &[(1, 100)]).await;
    let (router, _) = router(&pool, 1);
    let (status, text) = post(
        router,
        "rawRollbackSwallowed",
        r#"{"args":{"accountId":1,"amount":100}}"#,
    )
    .await;
    let after = balances(&pool).await;
    println!("rawRollbackSwallowed: {status} {text} balances={after:?}");
    assert_eq!(after, vec![(1, 100)], "the raw ROLLBACK undid the debit");
    assert_ne!(
        status,
        StatusCode::OK,
        "200 for a debit that did not happen: {text}"
    );
}
