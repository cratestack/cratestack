//! GHSA-r67q-4qqq-g9gm, what an `@isolation` retry loop treats as its own
//! (docs/design/procedure-isolation.md §5, §6, §7.1):
//!
//! - a typed SQLSTATE is authoritative: request data echoed into a `P0001`
//!   or `22P02` message that contains `40001` is not a serialization
//!   failure — the body runs once, the real error is returned, and it is
//!   recorded under its `Idempotency-Key`;
//! - `TRANSACTION_ABORTED` is final: another transaction's exhausted abort,
//!   propagated by an `@isolation` body, is not retried by the outer loop;
//! - only the owner answers `TRANSACTION_ABORTED`: a procedure that
//!   propagates another's abort (with or without `@isolation`, work
//!   committed on the pool or not) answers `500 INTERNAL_ERROR`, recorded
//!   under its key;
//! - a nested `@isolation` call joins the outer attempt (one transaction,
//!   nothing committed on its own, no duplicate on retry), a stricter
//!   nested level is refused, and a second joined call running at the same
//!   time, or a joined call cancelled half-way, poisons the attempt.
//!
//! REST and RPC, from one fixture body. Needs a database: `just test-ci-db
//! --test procedure_isolation_nested -- --test-threads=1` with
//! `CRATESTACK_REQUIRE_DB=1`.

#![cfg(feature = "codec-json")]

mod support;

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
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
    /// Runs of the procedure a test calls (the outer one).
    runs: AtomicUsize,
    /// Runs of `alwaysConflict`, `creditRead` and `creditStrict`.
    inner_runs: AtomicUsize,
    resolver_runs: AtomicUsize,
    /// The `innerTx` resolver calls `creditStrict` instead of `creditRead`.
    strict: AtomicBool,
    /// The `innerTx` resolver hits a real 40001 once, after its nested call.
    conflict_once: AtomicBool,
    /// What `propagateAbort` builds its pool-backed handle from.
    pool: OnceLock<PgPool>,
    /// The operator detail of the nested call's error, if it failed.
    nested_error: std::sync::Mutex<Option<String>>,
    /// The `innerTx` resolver joins `creditAfterPeer` and `failAfterPeer`
    /// concurrently instead of calling `creditRead`.
    concurrent: AtomicBool,
    /// The `innerTx` resolver calls `creditAfterPeer` under a timeout that
    /// cancels it after it credited, and answers anyway.
    cancel: AtomicBool,
    /// The `innerTx` resolver calls `debitThenFail` and swallows its error.
    audit_join: AtomicBool,
    /// Primary keys of the `AuditSink` events dispatched after commit.
    audited: std::sync::Mutex<Vec<String>>,
    /// `creditAfterPeer`'s body began; `failAfterPeer` is called after.
    first_began: tokio::sync::Notify,
    /// `failAfterPeer` began (or finished); `creditAfterPeer` credits after.
    peer_started: tokio::sync::Notify,
    /// `creditAfterPeer` credited; `failAfterPeer` fails after.
    credited: tokio::sync::Notify,
    /// `failAfterPeer`'s call finished; `creditAfterPeer` returns after.
    peer_closed: tokio::sync::Notify,
}

/// The `AuditSink` every router installs: records what reached it.
struct AuditRecorder(Arc<Shared>);

#[async_trait::async_trait]
impl cratestack::AuditSink for AuditRecorder {
    async fn record(&self, event: &cratestack::AuditEvent) -> Result<(), CratestackError> {
        self.0
            .audited
            .lock()
            .unwrap()
            .push(event.primary_key.to_string());
        Ok(())
    }
}

/// How long a concurrent-join fixture waits for its peer before going on.
const PEER_WAIT: Duration = Duration::from_millis(500);

fn db_err(error: sqlx::Error) -> CratestackError {
    cratestack::cratestack_error_from_sqlx(error)
}

const RAISE_40001: &str =
    "DO $$ BEGIN RAISE EXCEPTION 'forced' USING ERRCODE = 'serialization_failure'; END $$";
const TX_AND_LEVEL: &str =
    "SELECT txid_current()::text || ' ' || current_setting('transaction_isolation')";

macro_rules! nested_impl {
    () => {
        use super::{PEER_WAIT, RAISE_40001, Shared, TX_AND_LEVEL, db_err};
        use cratestack::sqlx;
        use cratestack::{CratestackContext, CratestackError};
        use cratestack_schema::procedures as p;
        use cratestack_schema::{Cratestack, IsolatedCratestack, Joined, Probe, Report, Transfer};
        use std::sync::Arc;
        use std::sync::atomic::Ordering;

        #[derive(Clone)]
        pub struct Procedures(pub Arc<Shared>);

        #[derive(Clone)]
        pub struct Resolvers(pub Arc<Shared>);

        /// `alwaysConflict` through its `invoke_with_db`, on `db`.
        async fn call_always_conflict(
            db: &Cratestack,
            registry: &Procedures,
            ctx: &CratestackContext,
        ) -> Result<Report, CratestackError> {
            let args = p::always_conflict::Args {
                args: Probe {
                    nonce: "inner".into(),
                },
            };
            let (registry, call_args, call_ctx) = (registry.clone(), args.clone(), ctx.clone());
            p::always_conflict::invoke_with_db(
                db,
                &args,
                ctx,
                move |tx_db, authorized| async move {
                    p::ProcedureRegistry::always_conflict(
                        &registry, &tx_db, &call_ctx, call_args, authorized,
                    )
                    .await
                },
            )
            .await
        }

        /// Debit `account` by `amount` through the model builders.
        macro_rules! debit {
            ($db:ident, $ctx:ident, $account:expr, $amount:expr) => {{
                let account = $db
                    .nest_account()
                    .find_unique($account)
                    .run($ctx)
                    .await?
                    .ok_or_else(|| CratestackError::NotFound("no account".into()))?;
                $db.nest_account()
                    .update($account)
                    .set(cratestack_schema::UpdateNestAccountInput {
                        balance: Some(account.balance - $amount),
                    })
                    .run($ctx)
                    .await?
            }};
        }

        /// Credit through raw SQL, answering the transaction's id and level.
        async fn credit(
            shared: &Shared,
            db: &IsolatedCratestack,
            args: p::credit_read::Args,
        ) -> Result<Report, CratestackError> {
            shared.inner_runs.fetch_add(1, Ordering::SeqCst);
            let transfer = args.args;
            db.transaction(async |tx| {
                sqlx::query("UPDATE nest_accounts SET balance = balance + $1 WHERE id = $2")
                    .bind(transfer.amount)
                    .bind(transfer.peerId)
                    .execute(&mut ***tx)
                    .await
                    .map_err(db_err)?;
                let level: String = sqlx::query_scalar(TX_AND_LEVEL)
                    .fetch_one(&mut ***tx)
                    .await
                    .map_err(db_err)?;
                Ok(Report { level })
            })
            .await
        }

        impl p::ProcedureRegistry for Procedures {
            async fn raise_echo(
                &self,
                db: &IsolatedCratestack,
                _ctx: &CratestackContext,
                args: p::raise_echo::Args,
                _authorized: p::raise_echo::Authorized,
            ) -> Result<Report, CratestackError> {
                self.0.runs.fetch_add(1, Ordering::SeqCst);
                let nonce = args.args.nonce;
                db.transaction(async |tx| {
                    sqlx::query("SELECT nest_refuse($1)")
                        .bind(nonce)
                        .execute(&mut ***tx)
                        .await
                        .map_err(db_err)?;
                    Ok(Report {
                        level: "unreachable".into(),
                    })
                })
                .await
            }

            async fn cast_echo(
                &self,
                db: &IsolatedCratestack,
                _ctx: &CratestackContext,
                args: p::cast_echo::Args,
                _authorized: p::cast_echo::Authorized,
            ) -> Result<Report, CratestackError> {
                self.0.runs.fetch_add(1, Ordering::SeqCst);
                let nonce = args.args.nonce;
                db.transaction(async |tx| {
                    let value: i64 = sqlx::query_scalar("SELECT ($1::text)::bigint")
                        .bind(nonce)
                        .fetch_one(&mut ***tx)
                        .await
                        .map_err(db_err)?;
                    Ok(Report {
                        level: value.to_string(),
                    })
                })
                .await
            }

            async fn always_conflict(
                &self,
                db: &IsolatedCratestack,
                _ctx: &CratestackContext,
                _args: p::always_conflict::Args,
                _authorized: p::always_conflict::Authorized,
            ) -> Result<Report, CratestackError> {
                self.0.inner_runs.fetch_add(1, Ordering::SeqCst);
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

            async fn propagate_abort(
                &self,
                _db: &IsolatedCratestack,
                ctx: &CratestackContext,
                _args: p::propagate_abort::Args,
                _authorized: p::propagate_abort::Authorized,
            ) -> Result<Report, CratestackError> {
                self.0.runs.fetch_add(1, Ordering::SeqCst);
                // A pool-backed handle the implementor holds itself: outside
                // the attempt (§11), so `alwaysConflict` owns its own loop.
                let pool = self.0.pool.get().expect("pool").clone();
                let own = Cratestack::builder(pool)
                    .with_isolation_max_retries(0)
                    .build();
                call_always_conflict(&own, self, ctx).await
            }

            async fn debit_then_abort(
                &self,
                db: &Cratestack,
                ctx: &CratestackContext,
                args: p::debit_then_abort::Args,
                _authorized: p::debit_then_abort::Authorized,
            ) -> Result<Report, CratestackError> {
                self.0.runs.fetch_add(1, Ordering::SeqCst);
                let transfer = args.args;
                debit!(db, ctx, transfer.accountId, transfer.amount);
                call_always_conflict(db, self, ctx).await
            }

            async fn join_outer(
                &self,
                db: &IsolatedCratestack,
                ctx: &CratestackContext,
                args: p::join_outer::Args,
                _authorized: p::join_outer::Authorized,
            ) -> Result<Joined, CratestackError> {
                self.0.runs.fetch_add(1, Ordering::SeqCst);
                let transfer = args.args;
                debit!(db, ctx, transfer.accountId, transfer.amount);
                let outer_tx = db
                    .transaction(async |tx| {
                        sqlx::query_scalar::<_, String>(TX_AND_LEVEL)
                            .fetch_one(&mut ***tx)
                            .await
                            .map_err(db_err)
                    })
                    .await?;
                Ok(Joined {
                    accountId: transfer.accountId,
                    peerId: transfer.peerId,
                    amount: transfer.amount,
                    outerTx: outer_tx,
                })
            }

            async fn credit_read(
                &self,
                db: &IsolatedCratestack,
                _ctx: &CratestackContext,
                args: p::credit_read::Args,
                _authorized: p::credit_read::Authorized,
            ) -> Result<Report, CratestackError> {
                credit(&self.0, db, args).await
            }

            async fn credit_strict(
                &self,
                db: &IsolatedCratestack,
                _ctx: &CratestackContext,
                args: p::credit_strict::Args,
                _authorized: p::credit_strict::Authorized,
            ) -> Result<Report, CratestackError> {
                let args = p::credit_read::Args { args: args.args };
                credit(&self.0, db, args).await
            }

            async fn credit_after_peer(
                &self,
                db: &IsolatedCratestack,
                _ctx: &CratestackContext,
                args: p::credit_after_peer::Args,
                _authorized: p::credit_after_peer::Authorized,
            ) -> Result<Report, CratestackError> {
                let shared = &self.0;
                shared.first_began.notify_one();
                let _ = tokio::time::timeout(PEER_WAIT, shared.peer_started.notified()).await;
                let report = credit(shared, db, p::credit_read::Args { args: args.args }).await?;
                shared.credited.notify_one();
                let _ = tokio::time::timeout(PEER_WAIT, shared.peer_closed.notified()).await;
                Ok(report)
            }

            async fn cancel_transaction(
                &self,
                db: &IsolatedCratestack,
                _ctx: &CratestackContext,
                args: p::cancel_transaction::Args,
                _authorized: p::cancel_transaction::Authorized,
            ) -> Result<Report, CratestackError> {
                self.0.runs.fetch_add(1, Ordering::SeqCst);
                let transfer = args.args;
                let credit = db.transaction(async |tx| {
                    sqlx::query("UPDATE nest_accounts SET balance = balance + $1 WHERE id = $2")
                        .bind(transfer.amount)
                        .bind(transfer.peerId)
                        .execute(&mut ***tx)
                        .await
                        .map_err(db_err)?;
                    tokio::time::sleep(PEER_WAIT).await;
                    Ok(())
                });
                let outcome = tokio::time::timeout(PEER_WAIT / 5, credit).await;
                Ok(Report {
                    level: format!("cancelled: {}", outcome.is_err()),
                })
            }

            async fn debit_then_fail(
                &self,
                db: &IsolatedCratestack,
                ctx: &CratestackContext,
                args: p::debit_then_fail::Args,
                _authorized: p::debit_then_fail::Authorized,
            ) -> Result<Report, CratestackError> {
                self.0.inner_runs.fetch_add(1, Ordering::SeqCst);
                let transfer = args.args;
                debit!(db, ctx, transfer.peerId, transfer.amount);
                Err(CratestackError::Validation(
                    "fails after an audited write".into(),
                ))
            }

            async fn fail_after_peer(
                &self,
                _db: &IsolatedCratestack,
                _ctx: &CratestackContext,
                _args: p::fail_after_peer::Args,
                _authorized: p::fail_after_peer::Authorized,
            ) -> Result<Report, CratestackError> {
                self.0.peer_started.notify_one();
                let _ = tokio::time::timeout(PEER_WAIT, self.0.credited.notified()).await;
                Err(CratestackError::Validation(
                    "fails after its peer credited".into(),
                ))
            }
        }

        /// `creditAfterPeer` and `failAfterPeer` joined to `db`'s attempt at
        /// the same time; `failAfterPeer`'s error is swallowed, the way a
        /// resolver treating a field as optional would.
        async fn join_concurrently(
            db: &Cratestack,
            shared: &Arc<Shared>,
            ctx: &CratestackContext,
            transfer: Transfer,
        ) -> Result<String, CratestackError> {
            let registry = Procedures(shared.clone());
            let credit_args = p::credit_after_peer::Args {
                args: transfer.clone(),
            };
            let (call_registry, call_args, call_ctx) =
                (registry.clone(), credit_args.clone(), ctx.clone());
            let credit = p::credit_after_peer::invoke_with_db(
                db,
                &credit_args,
                ctx,
                move |tx_db, authorized| async move {
                    p::ProcedureRegistry::credit_after_peer(
                        &call_registry,
                        &tx_db,
                        &call_ctx,
                        call_args,
                        authorized,
                    )
                    .await
                },
            );
            let fail_args = p::fail_after_peer::Args { args: transfer };
            let (call_registry, call_args, call_ctx) = (registry, fail_args.clone(), ctx.clone());
            let fail = async {
                // Not before `creditAfterPeer` holds its savepoint: an
                // operation racing its `SAVEPOINT` is refused as "in use".
                let _ = tokio::time::timeout(PEER_WAIT, shared.first_began.notified()).await;
                let result = p::fail_after_peer::invoke_with_db(
                    db,
                    &fail_args,
                    ctx,
                    move |tx_db, authorized| async move {
                        p::ProcedureRegistry::fail_after_peer(
                            &call_registry,
                            &tx_db,
                            &call_ctx,
                            call_args,
                            authorized,
                        )
                        .await
                    },
                )
                .await;
                shared.peer_started.notify_one();
                shared.peer_closed.notify_one();
                result
            };
            let (credited, failed) = tokio::join!(credit, fail);
            *shared.nested_error.lock().unwrap() = failed
                .err()
                .and_then(|error| error.detail().map(ToOwned::to_owned));
            Ok(credited?.level)
        }

        /// `creditAfterPeer` joined to `db`'s attempt and dropped by a
        /// timeout after it credited, while it waits for a peer that never
        /// comes; the resolver answers anyway.
        async fn join_then_cancel(
            db: &Cratestack,
            shared: &Arc<Shared>,
            ctx: &CratestackContext,
            transfer: Transfer,
        ) -> Result<String, CratestackError> {
            shared.peer_started.notify_one();
            let registry = Procedures(shared.clone());
            let args = p::credit_after_peer::Args { args: transfer };
            let (call_args, call_ctx) = (args.clone(), ctx.clone());
            let call = p::credit_after_peer::invoke_with_db(
                db,
                &args,
                ctx,
                move |tx_db, authorized| async move {
                    p::ProcedureRegistry::credit_after_peer(
                        &registry, &tx_db, &call_ctx, call_args, authorized,
                    )
                    .await
                },
            );
            let outcome = tokio::time::timeout(PEER_WAIT / 5, call).await;
            Ok(format!("cancelled: {}", outcome.is_err()))
        }

        impl cratestack_schema::ComputedFieldResolver for Resolvers {
            fn resolve_joined_inner_tx(
                &self,
                db: &Cratestack,
                source: &Joined,
                ctx: &CratestackContext,
            ) -> impl core::future::Future<Output = Result<String, CratestackError>> + Send {
                let (db, ctx, shared) = (db.clone(), ctx.clone(), self.0.clone());
                let transfer = Transfer {
                    accountId: source.accountId,
                    peerId: source.peerId,
                    amount: source.amount,
                };
                async move {
                    shared.resolver_runs.fetch_add(1, Ordering::SeqCst);
                    if shared.concurrent.load(Ordering::SeqCst) {
                        return join_concurrently(&db, &shared, &ctx, transfer).await;
                    }
                    if shared.cancel.load(Ordering::SeqCst) {
                        return join_then_cancel(&db, &shared, &ctx, transfer).await;
                    }
                    if shared.audit_join.load(Ordering::SeqCst) {
                        let args = p::debit_then_fail::Args { args: transfer };
                        let (registry, call_args, call_ctx) =
                            (Procedures(shared.clone()), args.clone(), ctx.clone());
                        let failed = p::debit_then_fail::invoke_with_db(
                            &db,
                            &args,
                            &ctx,
                            move |tx_db, authorized| async move {
                                p::ProcedureRegistry::debit_then_fail(
                                    &registry, &tx_db, &call_ctx, call_args, authorized,
                                )
                                .await
                            },
                        )
                        .await;
                        return Ok(format!("joined call failed: {}", failed.is_err()));
                    }
                    let registry = Procedures(shared.clone());
                    let call_ctx = ctx.clone();
                    // The resolver's `db` is bound to `joinOuter`'s attempt:
                    // the nested `@isolation` call joins it.
                    let report = if shared.strict.load(Ordering::SeqCst) {
                        let args = p::credit_strict::Args { args: transfer };
                        let call_args = args.clone();
                        p::credit_strict::invoke_with_db(
                            &db,
                            &args,
                            &ctx,
                            move |tx_db, authorized| async move {
                                p::ProcedureRegistry::credit_strict(
                                    &registry, &tx_db, &call_ctx, call_args, authorized,
                                )
                                .await
                            },
                        )
                        .await
                        .inspect_err(|error| {
                            *shared.nested_error.lock().unwrap() =
                                error.detail().map(ToOwned::to_owned);
                        })?
                    } else {
                        let args = p::credit_read::Args { args: transfer };
                        let call_args = args.clone();
                        p::credit_read::invoke_with_db(
                            &db,
                            &args,
                            &ctx,
                            move |tx_db, authorized| async move {
                                p::ProcedureRegistry::credit_read(
                                    &registry, &tx_db, &call_ctx, call_args, authorized,
                                )
                                .await
                            },
                        )
                        .await?
                    };
                    if shared.conflict_once.swap(false, Ordering::SeqCst) {
                        db.transaction(async |tx| {
                            sqlx::query(RAISE_40001)
                                .execute(&mut ***tx)
                                .await
                                .map(|_| ())
                                .map_err(db_err)
                        })
                        .await?;
                    }
                    Ok(report.level)
                }
            }
        }
    };
}

pub mod rest {
    use cratestack::include_server_schema;
    include_server_schema!(
        "tests/fixtures/procedure_isolation_nested.cstack",
        db = Postgres
    );
    nested_impl!();
}

pub mod rpc {
    use cratestack::include_server_schema;
    include_server_schema!(
        "tests/fixtures/procedure_isolation_nested_rpc.cstack",
        db = Postgres
    );
    nested_impl!();
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
    ($module:ident, $mount:ident, $pool:expr, $shared:expr, $retries:expr) => {{
        let db = $module::cratestack_schema::Cratestack::builder($pool.clone())
            .with_isolation_max_retries($retries)
            .with_audit_sink(Arc::new(AuditRecorder($shared.clone())))
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

/// A router with the given retry budget, behind an `IdempotencyLayer` over
/// the real Postgres store.
fn router(pool: &PgPool, rpc: bool, retries: u32) -> (cratestack::axum::Router, Arc<Shared>) {
    let shared = Arc::new(Shared::default());
    shared.pool.set(pool.clone()).expect("fresh");
    let router = if rpc {
        mount!(rpc, rpc_router, pool, shared, retries)
    } else {
        mount!(rest, router, pool, shared, retries)
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
    let peer: std::net::SocketAddr = "192.0.2.94:1".parse().unwrap();
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
        // `a_read_error_echoing_request_data_is_not_retried` replaces the
        // table with a view.
        "DO $$ BEGIN IF EXISTS (SELECT 1 FROM pg_views WHERE viewname = 'nest_accounts') \
         THEN DROP VIEW nest_accounts; END IF; END $$",
        "DROP TABLE IF EXISTS nest_raw",
        "DROP TABLE IF EXISTS nest_accounts",
        "CREATE TABLE nest_accounts (id BIGINT PRIMARY KEY, balance BIGINT NOT NULL)",
        "INSERT INTO nest_accounts VALUES (1, 100), (2, 500)",
        "CREATE OR REPLACE FUNCTION nest_refuse(requested text) RETURNS void \
         LANGUAGE plpgsql AS $$ BEGIN \
         RAISE EXCEPTION 'insufficient funds: requested %', requested; END $$",
    ] {
        sqlx::query(sqlx::AssertSqlSafe(statement.to_owned()))
            .execute(pool)
            .await
            .unwrap_or_else(|error| panic!("{statement}: {error}"));
    }
}

async fn balances(pool: &PgPool) -> (i64, i64) {
    let rows: Vec<i64> = sqlx::query_scalar("SELECT balance FROM nest_accounts ORDER BY id")
        .fetch_all(pool)
        .await
        .unwrap();
    (rows[0], rows[1])
}

fn code(rpc: bool, rest: &'static str, rpc_code: &'static str) -> String {
    format!(r#""code":"{}""#, if rpc { rpc_code } else { rest })
}

const PROBE_40001: &str = r#"{"args":{"nonce":"40001"}}"#;
const PROBE_40001X: &str = r#"{"args":{"nonce":"40001x"}}"#;
const TRANSFER: &str = r#"{"args":{"accountId":1,"peerId":2,"amount":10}}"#;

/// Decided change 1. Request data echoed into a database error of another
/// SQLSTATE — `RAISE EXCEPTION 'insufficient funds: requested %'` (P0001),
/// a cast of `'40001x'` to bigint (22P02) — reads `40001` in its text. With
/// a budget of 3 retries the body still runs once, the caller gets the real
/// error (a 500 `DATABASE_ERROR`, not a 409 `TRANSACTION_ABORTED`), and the
/// response is recorded: the same key replays it instead of running again.
#[tokio::test]
async fn request_data_echoed_into_a_typed_error_is_not_retried() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool.clone();
    for (label, rpc) in TRANSPORTS {
        for (procedure, body) in [("raiseEcho", PROBE_40001), ("castEcho", PROBE_40001X)] {
            reset(&pool).await;
            let (router, shared) = router(&pool, rpc, 3);
            let key = format!("k-{procedure}");
            let first = post(&router, &uri(rpc, procedure), Some(&key), body).await;
            let second = post(&router, &uri(rpc, procedure), Some(&key), body).await;
            println!("{label} {procedure}: {first:?} / {second:?}");
            assert_eq!(
                first.0,
                StatusCode::INTERNAL_SERVER_ERROR,
                "{label} {procedure}: {first:?}"
            );
            assert!(
                first.1.contains(&code(rpc, "DATABASE_ERROR", "internal")),
                "{label} {procedure}: {first:?}"
            );
            assert!(
                !first.1.contains("aborted") && !first.1.contains("TRANSACTION_ABORTED"),
                "{label} {procedure}: {first:?}"
            );
            assert_eq!(
                shared.runs.load(Ordering::SeqCst),
                1,
                "{label} {procedure}: the body ran once, over both calls"
            );
            assert_eq!(first, second, "{label} {procedure}: the key replayed it");
        }
    }
}

/// Decided change 2. `propagateAbort` (an `@isolation` procedure, budget 2)
/// gets `alwaysConflict`'s exhausted abort from its own transaction and
/// returns it. The outer loop does not run again because of it — it is
/// final. The outer loop did not run out of retries, so its dispatch does
/// not own that abort: it answers `500 INTERNAL_ERROR`, not
/// `TRANSACTION_ABORTED`, and the response is recorded under its key.
#[tokio::test]
async fn an_exhausted_inner_abort_is_final_for_the_outer_loop() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool.clone();
    for (label, rpc) in TRANSPORTS {
        reset(&pool).await;
        let (router, shared) = router(&pool, rpc, 2);
        let target = uri(rpc, "propagateAbort");
        let body = r#"{"args":{"nonce":"x"}}"#;
        let first = post(&router, &target, Some("k-propagated"), body).await;
        println!("{label} propagateAbort: {first:?}");
        assert_eq!(
            first.0,
            StatusCode::INTERNAL_SERVER_ERROR,
            "{label}: {first:?}"
        );
        assert!(
            first.1.contains(&code(rpc, "INTERNAL_ERROR", "internal")),
            "{label}: not the owner, so not TRANSACTION_ABORTED: {first:?}"
        );
        assert_eq!(
            shared.runs.load(Ordering::SeqCst),
            1,
            "{label}: not retried"
        );
        assert_eq!(shared.inner_runs.load(Ordering::SeqCst), 1, "{label}");
        let second = post(&router, &target, Some("k-propagated"), body).await;
        assert_eq!(first, second, "{label}: recorded and replayed");
        assert_eq!(shared.runs.load(Ordering::SeqCst), 1, "{label}: a replay");
    }
}

/// Decided change 3, idempotency. `debitThenAbort` has no `@isolation`: it
/// commits a debit on the pool, then propagates `alwaysConflict`'s
/// exhausted abort. It is not that abort's owner: it answers `500
/// INTERNAL_ERROR` — `TRANSACTION_ABORTED` would tell the client nothing was
/// committed and to send it again, and a retry under a new key would debit
/// twice — and the response is recorded like any other error: sending the
/// same key again must not debit again.
#[tokio::test]
async fn a_non_owner_propagating_an_abort_is_recorded() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool.clone();
    for (label, rpc) in TRANSPORTS {
        reset(&pool).await;
        let (router, shared) = router(&pool, rpc, 0);
        let target = uri(rpc, "debitThenAbort");
        let first = post(&router, &target, Some("k-non-owner"), TRANSFER).await;
        let second = post(&router, &target, Some("k-non-owner"), TRANSFER).await;
        println!("{label} debitThenAbort: {first:?} / {second:?}");
        assert_eq!(
            first.0,
            StatusCode::INTERNAL_SERVER_ERROR,
            "{label}: {first:?}"
        );
        assert!(
            first.1.contains(&code(rpc, "INTERNAL_ERROR", "internal")),
            "{label}: not \"nothing committed, send it again\": {first:?}"
        );
        assert_eq!(first, second, "{label}: the key replayed the response");
        assert_eq!(shared.runs.load(Ordering::SeqCst), 1, "{label}: ran once");
        assert_eq!(balances(&pool).await, (90, 500), "{label}: debited once");

        // The control: the owner's own exhausted abort is still released.
        let owner = uri(rpc, "alwaysConflict");
        let body = r#"{"args":{"nonce":"x"}}"#;
        post(&router, &owner, Some("k-owner"), body).await;
        post(&router, &owner, Some("k-owner"), body).await;
        assert_eq!(
            shared.inner_runs.load(Ordering::SeqCst),
            3,
            "{label}: one run inside debitThenAbort, two under the owner's released key"
        );
    }
}

/// Decided change 3, joining. `joinOuter` debits account 1; its `innerTx`
/// resolver credits account 2 through `creditRead`'s `invoke_with_db` with
/// the resolver's attempt-bound handle, then (once) hits a real 40001. The
/// nested call runs in the outer transaction (same transaction id, at the
/// outer level), commits nothing on its own, and the retried attempt
/// credits once — not once per attempt.
#[tokio::test]
async fn a_nested_isolated_call_joins_the_outer_attempt() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool.clone();
    for (label, rpc) in TRANSPORTS {
        for conflict in [false, true] {
            reset(&pool).await;
            let (router, shared) = router(&pool, rpc, 3);
            shared.conflict_once.store(conflict, Ordering::SeqCst);
            let (status, text) = post(&router, &uri(rpc, "joinOuter"), None, TRANSFER).await;
            println!("{label} joinOuter conflict={conflict}: {status} {text}");
            assert_eq!(status, StatusCode::OK, "{label}: {text}");
            let body: serde_json::Value = serde_json::from_str(&text).unwrap();
            let outer = body["outerTx"].as_str().unwrap();
            let inner = body["innerTx"].as_str().unwrap();
            assert_eq!(outer, inner, "{label}: one transaction, one level");
            assert!(outer.ends_with(" repeatable read"), "{label}: {outer}");
            let attempts = if conflict { 2 } else { 1 };
            assert_eq!(shared.runs.load(Ordering::SeqCst), attempts, "{label}");
            assert_eq!(
                shared.inner_runs.load(Ordering::SeqCst),
                attempts,
                "{label}"
            );
            assert_eq!(
                balances(&pool).await,
                (90, 510),
                "{label} conflict={conflict}: debited and credited exactly once"
            );
        }
    }
}

/// Decided change 3, levels. A nested call declaring a stricter level than
/// the attempt it would join (`serializable` inside `repeatable read`) is
/// refused before its body runs; the refusal fails the outer attempt, which
/// is rolled back and not retried.
#[tokio::test]
async fn a_stricter_nested_level_is_refused() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool.clone();
    for (label, rpc) in TRANSPORTS {
        reset(&pool).await;
        let (router, shared) = router(&pool, rpc, 3);
        shared.strict.store(true, Ordering::SeqCst);
        let (status, text) = post(&router, &uri(rpc, "joinOuter"), None, TRANSFER).await;
        println!("{label} joinOuter strict: {status} {text}");
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{label}: {text}");
        assert!(
            text.contains(&code(rpc, "INTERNAL_ERROR", "internal")),
            "{label}: {text}"
        );
        let detail = shared
            .nested_error
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_default();
        assert!(
            detail.contains("isolation level cannot be raised"),
            "{label}: {detail}"
        );
        assert_eq!(shared.inner_runs.load(Ordering::SeqCst), 0, "{label}");
        assert_eq!(
            shared.runs.load(Ordering::SeqCst),
            1,
            "{label}: not retried"
        );
        assert_eq!(balances(&pool).await, (100, 500), "{label}: rolled back");
    }
}

/// Two nested calls joined to one attempt at the same time (`tokio::join!`
/// in the `innerTx` resolver): `creditAfterPeer` begins first and credits
/// account 2 after `failAfterPeer` has begun; `failAfterPeer` then fails and
/// the resolver swallows its error. Its savepoint's rollback would undo the
/// credit its peer made after it began, while the peer reported `Ok`, and
/// the attempt would commit the debit without the credit. The second call is
/// refused at its start and poisons the attempt instead: nothing commits.
#[tokio::test]
async fn concurrent_joined_calls_poison_the_attempt() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool.clone();
    for (label, rpc) in TRANSPORTS {
        reset(&pool).await;
        let (router, shared) = router(&pool, rpc, 3);
        shared.concurrent.store(true, Ordering::SeqCst);
        let (status, text) = post(&router, &uri(rpc, "joinOuter"), None, TRANSFER).await;
        println!("{label} joinOuter concurrent: {status} {text}");
        assert_eq!(
            balances(&pool).await,
            (100, 500),
            "{label}: nothing committed — not a debit without the credit its joined call reported"
        );
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{label}: {text}");
        assert!(
            text.contains(&code(rpc, "INTERNAL_ERROR", "internal")),
            "{label}: {text}"
        );
        let detail = shared
            .nested_error
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_default();
        assert!(detail.contains("one at a time"), "{label}: {detail}");
        assert_eq!(
            shared.runs.load(Ordering::SeqCst),
            1,
            "{label}: not retried"
        );
    }
}

/// A nested call joined to the attempt and dropped before it finished (the
/// `innerTx` resolver's timeout cancels `creditAfterPeer` after it credited)
/// leaves its savepoint open with what it wrote. The resolver answers
/// anyway; the attempt is poisoned rather than committing the half of a
/// call that never returned.
#[tokio::test]
async fn a_cancelled_joined_call_poisons_the_attempt() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool.clone();
    for (label, rpc) in TRANSPORTS {
        reset(&pool).await;
        let (router, shared) = router(&pool, rpc, 3);
        shared.cancel.store(true, Ordering::SeqCst);
        let (status, text) = post(&router, &uri(rpc, "joinOuter"), None, TRANSFER).await;
        println!("{label} joinOuter cancel: {status} {text}");
        assert_eq!(
            shared.inner_runs.load(Ordering::SeqCst),
            1,
            "{label}: it credited"
        );
        assert_eq!(
            balances(&pool).await,
            (100, 500),
            "{label}: nothing committed"
        );
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{label}: {text}");
        assert_eq!(
            shared.runs.load(Ordering::SeqCst),
            1,
            "{label}: not retried"
        );
    }
}

/// The typed-SQLSTATE rule reaches the framework's own reads. The model's
/// table is a view that casts a stored `'40001x'` to `bigint`, so
/// `joinOuter`'s `find_unique` fails with `22P02` — `invalid input syntax
/// for type bigint: "40001x"` — whose text contains `40001`. The read
/// builders used to turn every sqlx error into the untyped
/// `Database(String)`, which is classified by its text: the body ran up to
/// the budget, answered `409 TRANSACTION_ABORTED` and released the key.
/// Typed, it runs once and the real 500 is recorded under its key.
#[tokio::test]
async fn a_read_error_echoing_request_data_is_not_retried() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool.clone();
    for (label, rpc) in TRANSPORTS {
        reset(&pool).await;
        for statement in [
            "DROP TABLE nest_accounts",
            "CREATE TABLE nest_raw (id BIGINT PRIMARY KEY, balance TEXT NOT NULL)",
            "INSERT INTO nest_raw VALUES (1, '40001x'), (2, '500')",
            "CREATE VIEW nest_accounts AS SELECT id, balance::bigint AS balance FROM nest_raw",
        ] {
            sqlx::query(sqlx::AssertSqlSafe(statement.to_owned()))
                .execute(&pool)
                .await
                .unwrap_or_else(|error| panic!("{statement}: {error}"));
        }
        let (router, shared) = router(&pool, rpc, 3);
        let target = uri(rpc, "joinOuter");
        let first = post(&router, &target, Some("k-read-echo"), TRANSFER).await;
        let second = post(&router, &target, Some("k-read-echo"), TRANSFER).await;
        println!("{label} joinOuter read echo: {first:?} / {second:?}");
        assert_eq!(
            shared.runs.load(Ordering::SeqCst),
            1,
            "{label}: the body ran once, over both calls"
        );
        assert_eq!(
            first.0,
            StatusCode::INTERNAL_SERVER_ERROR,
            "{label}: {first:?}"
        );
        assert!(
            first.1.contains(&code(rpc, "DATABASE_ERROR", "internal")),
            "{label}: {first:?}"
        );
        assert_eq!(first, second, "{label}: the key replayed it");
    }
    reset(&pool).await;
}

/// A joined call that fails is rolled back to its savepoint, and so are the
/// `AuditSink` events it queued. `joinOuter` debits account 1 (audited);
/// its `innerTx` resolver joins `debitThenFail`, which debits account 2
/// (audited) and fails, and swallows the error. The attempt keeps the outer
/// debit only, and the sink sees that one event, not an event for the debit
/// the rollback undid.
#[tokio::test]
async fn a_failed_joined_call_drops_its_audit_events() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool.clone();
    for (label, rpc) in TRANSPORTS {
        reset(&pool).await;
        let (router, shared) = router(&pool, rpc, 3);
        shared.audit_join.store(true, Ordering::SeqCst);
        let (status, text) = post(&router, &uri(rpc, "joinOuter"), None, TRANSFER).await;
        println!("{label} joinOuter audit: {status} {text}");
        assert_eq!(status, StatusCode::OK, "{label}: {text}");
        assert!(text.contains("joined call failed: true"), "{label}: {text}");
        assert_eq!(shared.inner_runs.load(Ordering::SeqCst), 1, "{label}");
        assert_eq!(
            balances(&pool).await,
            (90, 500),
            "{label}: outer debit only"
        );
        let audited = shared.audited.lock().unwrap().clone();
        assert_eq!(audited, vec!["1".to_owned()], "{label}: {audited:?}");
    }
}

/// `db.transaction(..)` dropped before it finished — `cancelTransaction`'s
/// own timeout cancels it after its credit, while it waits — leaves its
/// savepoint open with that credit. The body answers `Ok` anyway; the
/// attempt is poisoned rather than committing the half of a closure that
/// never returned.
#[tokio::test]
async fn a_cancelled_nested_transaction_poisons_the_attempt() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = test_pg.pool.clone();
    for (label, rpc) in TRANSPORTS {
        reset(&pool).await;
        let (router, shared) = router(&pool, rpc, 3);
        let target = uri(rpc, "cancelTransaction");
        let (status, text) = post(&router, &target, None, TRANSFER).await;
        println!("{label} cancelTransaction: {status} {text}");
        assert_eq!(balances(&pool).await, (100, 500), "{label}: nothing kept");
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{label}: {text}");
        assert_eq!(
            shared.runs.load(Ordering::SeqCst),
            1,
            "{label}: not retried"
        );
    }
}
