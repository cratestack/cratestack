//! Shared harness for `procedure_isolation.rs`: the two generated schemas
//! (REST and RPC, from the same fixture body), one `ProcedureRegistry`
//! implementation written the way an application author would, and the
//! database helpers.

#![allow(dead_code)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::extract::ConnectInfo;
use cratestack::axum::http::{Request, StatusCode};
use cratestack::sqlx::{self, PgPool};
use cratestack::{
    AuditEvent, AuditSink, AuthProvider, CratestackContext, CratestackError, RequestContext, Value,
};
use cratestack_codec_json::JsonCodec;
use tower::util::ServiceExt;

/// Holds each caller at the point between its read and its write until
/// `parties` callers have arrived, so two concurrent calls interleave the
/// way a race needs to. A retried attempt arrives again and passes straight
/// through (the count is already met), so a retry never waits for a
/// partner that is not coming.
pub struct Gate {
    parties: usize,
    arrived: AtomicUsize,
}

impl Gate {
    pub fn new(parties: usize) -> Arc<Self> {
        Arc::new(Self {
            parties,
            arrived: AtomicUsize::new(0),
        })
    }

    pub async fn wait(&self) {
        self.arrived.fetch_add(1, Ordering::SeqCst);
        let deadline = Instant::now() + Duration::from_secs(5);
        while self.arrived.load(Ordering::SeqCst) < self.parties && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
}

/// What `writeThenFail` observed from inside its own transaction.
#[derive(Debug, Clone, Default)]
pub struct Seen {
    pub model_saw_70: Option<bool>,
    pub raw_count: Option<i64>,
    pub reentrant_code: Option<String>,
}

pub struct Shared {
    runs: AtomicUsize,
    delivered: Arc<AtomicUsize>,
    gate: Arc<Gate>,
    seen: Mutex<Seen>,
}

impl Shared {
    pub fn runs(&self) -> usize {
        self.runs.load(Ordering::SeqCst)
    }

    /// `@@emit(updated)` events delivered to a subscriber.
    pub fn delivered(&self) -> usize {
        self.delivered.load(Ordering::SeqCst)
    }

    pub fn seen(&self) -> Seen {
        self.seen.lock().unwrap().clone()
    }
}

#[derive(Clone, Default)]
pub struct RecordingSink {
    events: Arc<Mutex<Vec<AuditEvent>>>,
}

impl RecordingSink {
    pub fn len(&self) -> usize {
        self.events.lock().unwrap().len()
    }
}

#[async_trait::async_trait]
impl AuditSink for RecordingSink {
    async fn record(&self, event: &AuditEvent) -> Result<(), CratestackError> {
        self.events.lock().unwrap().push(event.clone());
        Ok(())
    }
}

#[derive(Clone)]
pub struct AlwaysAuth;

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

fn db_err(error: sqlx::Error) -> CratestackError {
    cratestack::cratestack_error_from_sqlx(error)
}

const LEVEL_SQL: &str = "SELECT current_setting('transaction_isolation')";

/// The body every `ProcedureRegistry` below runs, written against whichever
/// handle the generated trait hands it. Macro so the REST and RPC schemas
/// (two distinct generated modules) share one implementation.
macro_rules! procedures_impl {
    () => {
        use super::{LEVEL_SQL, Shared, db_err};
        use cratestack::sqlx;
        use cratestack::{CratestackContext, CratestackError};
        use cratestack_schema::procedures as p;
        use cratestack_schema::{Cratestack, IsolatedCratestack};
        use std::sync::Arc;
        use std::sync::atomic::Ordering;

        #[derive(Clone)]
        pub struct Procedures(pub Arc<Shared>);

        async fn level(
            db: &IsolatedCratestack,
        ) -> Result<cratestack_schema::Report, CratestackError> {
            let level = db
                .transaction(async |tx| {
                    sqlx::query_scalar::<_, String>(LEVEL_SQL)
                        .fetch_one(&mut ***tx)
                        .await
                        .map_err(db_err)
                })
                .await?;
            Ok(cratestack_schema::Report { level })
        }

        macro_rules! withdraw_body {
            ($self:ident, $db:ident, $ctx:ident, $args:ident) => {{
                $self.0.runs.fetch_add(1, Ordering::SeqCst);
                let id = $args.args.accountId;
                let amount = $args.args.amount;
                let account = $db
                    .iso_account()
                    .find_unique(id)
                    .run($ctx)
                    .await?
                    .ok_or_else(|| CratestackError::NotFound("no account".into()))?;
                $self.0.gate.wait().await;
                if account.balance < amount {
                    return Err(CratestackError::Validation("insufficient funds".into()));
                }
                let updated = $db
                    .iso_account()
                    .update(id)
                    .set(cratestack_schema::UpdateIsoAccountInput {
                        balance: Some(account.balance - amount),
                    })
                    .run($ctx)
                    .await?;
                Ok(cratestack_schema::Receipt {
                    before: account.balance,
                    after: updated.balance,
                })
            }};
        }

        impl p::ProcedureRegistry for Procedures {
            async fn level_serializable(
                &self,
                db: &IsolatedCratestack,
                _ctx: &CratestackContext,
                _args: p::level_serializable::Args,
                _authorized: p::level_serializable::Authorized,
            ) -> Result<p::level_serializable::Output, CratestackError> {
                level(db).await
            }

            async fn level_repeatable_read(
                &self,
                db: &IsolatedCratestack,
                _ctx: &CratestackContext,
                _args: p::level_repeatable_read::Args,
                _authorized: p::level_repeatable_read::Authorized,
            ) -> Result<p::level_repeatable_read::Output, CratestackError> {
                level(db).await
            }

            async fn level_read_committed(
                &self,
                db: &IsolatedCratestack,
                _ctx: &CratestackContext,
                _args: p::level_read_committed::Args,
                _authorized: p::level_read_committed::Authorized,
            ) -> Result<p::level_read_committed::Output, CratestackError> {
                level(db).await
            }

            async fn level_plain(
                &self,
                db: &Cratestack,
                _ctx: &CratestackContext,
                _args: p::level_plain::Args,
                _authorized: p::level_plain::Authorized,
            ) -> Result<p::level_plain::Output, CratestackError> {
                let level = sqlx::query_scalar::<_, String>(LEVEL_SQL)
                    .fetch_one(db.pool())
                    .await
                    .map_err(db_err)?;
                Ok(cratestack_schema::Report { level })
            }

            async fn withdraw(
                &self,
                db: &IsolatedCratestack,
                ctx: &CratestackContext,
                args: p::withdraw::Args,
                _authorized: p::withdraw::Authorized,
            ) -> Result<p::withdraw::Output, CratestackError> {
                withdraw_body!(self, db, ctx, args)
            }

            async fn withdraw_plain(
                &self,
                db: &Cratestack,
                ctx: &CratestackContext,
                args: p::withdraw_plain::Args,
                _authorized: p::withdraw_plain::Authorized,
            ) -> Result<p::withdraw_plain::Output, CratestackError> {
                withdraw_body!(self, db, ctx, args)
            }

            async fn withdraw_swallow(
                &self,
                db: &IsolatedCratestack,
                ctx: &CratestackContext,
                args: p::withdraw_swallow::Args,
                _authorized: p::withdraw_swallow::Authorized,
            ) -> Result<p::withdraw_swallow::Output, CratestackError> {
                self.0.runs.fetch_add(1, Ordering::SeqCst);
                let id = args.args.accountId;
                let amount = args.args.amount;
                let account = db
                    .iso_account()
                    .find_unique(id)
                    .run(ctx)
                    .await?
                    .ok_or_else(|| CratestackError::NotFound("no account".into()))?;
                self.0.gate.wait().await;
                if account.balance < amount {
                    return Err(CratestackError::Validation("insufficient funds".into()));
                }
                let debit = db
                    .iso_account()
                    .update(id)
                    .set(cratestack_schema::UpdateIsoAccountInput {
                        balance: Some(account.balance - amount),
                    })
                    .run(ctx)
                    .await;
                // A careless author: the debit failed, report success anyway.
                let after = debit
                    .map(|updated| updated.balance)
                    .unwrap_or(account.balance);
                Ok(cratestack_schema::Receipt {
                    before: account.balance,
                    after,
                })
            }

            async fn withdraw_joint(
                &self,
                db: &IsolatedCratestack,
                ctx: &CratestackContext,
                args: p::withdraw_joint::Args,
                _authorized: p::withdraw_joint::Authorized,
            ) -> Result<p::withdraw_joint::Output, CratestackError> {
                self.0.runs.fetch_add(1, Ordering::SeqCst);
                let id = args.args.accountId;
                let amount = args.args.amount;
                let accounts = db.iso_account().find_many().run(ctx).await?;
                let total: i64 = accounts.iter().map(|account| account.balance).sum();
                let mine = accounts
                    .iter()
                    .find(|account| account.id == id)
                    .map(|account| account.balance)
                    .ok_or_else(|| CratestackError::NotFound("no account".into()))?;
                self.0.gate.wait().await;
                if total < amount {
                    return Err(CratestackError::Validation("insufficient funds".into()));
                }
                let updated = db
                    .iso_account()
                    .update(id)
                    .set(cratestack_schema::UpdateIsoAccountInput {
                        balance: Some(mine - amount),
                    })
                    .run(ctx)
                    .await?;
                Ok(cratestack_schema::Receipt {
                    before: mine,
                    after: updated.balance,
                })
            }

            async fn write_then_fail(
                &self,
                db: &IsolatedCratestack,
                ctx: &CratestackContext,
                args: p::write_then_fail::Args,
                _authorized: p::write_then_fail::Authorized,
            ) -> Result<p::write_then_fail::Output, CratestackError> {
                self.0.runs.fetch_add(1, Ordering::SeqCst);
                let id = args.args.accountId;
                db.iso_account()
                    .create(cratestack_schema::CreateIsoAccountInput {
                        id,
                        balance: args.args.amount,
                    })
                    .run(ctx)
                    .await?;
                let raw_id = id + 1;
                let composed_id = id + 2;
                let (raw_count, reentrant, audit_events) = db
                    .transaction(async |tx| {
                        sqlx::query("INSERT INTO iso_accounts (id, balance) VALUES ($1, 1)")
                            .bind(raw_id)
                            .execute(&mut ***tx)
                            .await
                            .map_err(db_err)?;
                        // A composed, audited write whose events the author
                        // hands to `dispatch_audit_sink` explicitly.
                        let composed = db
                            .iso_account()
                            .create(cratestack_schema::CreateIsoAccountInput {
                                id: composed_id,
                                balance: 1,
                            })
                            .run_in_tx(tx, ctx)
                            .await?;
                        let count: i64 = sqlx::query_scalar(
                            "SELECT COUNT(*)::BIGINT FROM iso_accounts WHERE id IN ($1, $2, $3)",
                        )
                        .bind(id)
                        .bind(raw_id)
                        .bind(composed_id)
                        .fetch_one(&mut ***tx)
                        .await
                        .map_err(db_err)?;
                        // Re-entrant use of the handle while `tx` holds it.
                        let reentrant = db.iso_account().find_unique(id).run(ctx).await;
                        Ok((
                            count,
                            reentrant.err().map(|error| error.code().to_owned()),
                            composed.audit_events,
                        ))
                    })
                    .await?;
                // Queued, not sent: this attempt is about to fail.
                db.dispatch_audit_sink(&audit_events).await;
                let model_saw = db.iso_account().find_unique(id).run(ctx).await?.is_some();
                *self.0.seen.lock().unwrap() = super::Seen {
                    model_saw_70: Some(model_saw),
                    raw_count: Some(raw_count),
                    reentrant_code: reentrant,
                };
                Err(CratestackError::Validation("forced failure".into()))
            }
        }
    };
}

pub mod rest {
    use cratestack::include_server_schema;
    include_server_schema!("tests/fixtures/procedure_isolation.cstack", db = Postgres);
    procedures_impl!();
}

pub mod rpc {
    use cratestack::include_server_schema;
    include_server_schema!(
        "tests/fixtures/procedure_isolation_rpc.cstack",
        db = Postgres
    );
    procedures_impl!();
}

fn shared(gate: Arc<Gate>) -> Arc<Shared> {
    Arc::new(Shared {
        runs: AtomicUsize::new(0),
        delivered: Arc::new(AtomicUsize::new(0)),
        gate,
        seen: Mutex::new(Seen::default()),
    })
}

pub fn routers(
    pool: &PgPool,
    gate: Arc<Gate>,
    sink: Option<RecordingSink>,
    rpc: bool,
) -> (cratestack::axum::Router, Arc<Shared>) {
    build(pool, gate, sink, None, rpc)
}

pub fn routers_with_retries(
    pool: &PgPool,
    gate: Arc<Gate>,
    retries: u32,
    rpc: bool,
) -> (cratestack::axum::Router, Arc<Shared>) {
    build(pool, gate, None, Some(retries), rpc)
}

macro_rules! build_router {
    ($module:ident, $mount:ident, $pool:expr, $shared:expr, $sink:expr, $retries:expr) => {{
        let mut builder = $module::cratestack_schema::Cratestack::builder($pool.clone());
        if let Some(sink) = $sink {
            builder = builder.with_audit_sink(Arc::new(sink));
        }
        if let Some(retries) = $retries {
            builder = builder.with_isolation_max_retries(retries);
        }
        let db = builder.build();
        let delivered = $shared.delivered.clone();
        db.events().on_iso_account_updated(move |_event| {
            let delivered = delivered.clone();
            async move {
                delivered.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        });
        $module::cratestack_schema::axum::$mount(
            db,
            $module::Procedures($shared),
            (),
            JsonCodec,
            AlwaysAuth,
            cratestack::DEFAULT_BODY_LIMIT_BYTES,
        )
    }};
}

fn build(
    pool: &PgPool,
    gate: Arc<Gate>,
    sink: Option<RecordingSink>,
    retries: Option<u32>,
    rpc: bool,
) -> (cratestack::axum::Router, Arc<Shared>) {
    let shared = shared(gate);
    let router = if rpc {
        build_router!(rpc, rpc_router, pool, shared.clone(), sink, retries)
    } else {
        build_router!(rest, router, pool, shared.clone(), sink, retries)
    };
    (router, shared)
}

pub async fn post(
    router: cratestack::axum::Router,
    uri: String,
    body: &str,
) -> (StatusCode, String) {
    let mut request = Request::post(uri.as_str())
        .header("content-type", "application/json")
        .header("accept", "application/json")
        .body(Body::from(body.to_owned()))
        .unwrap();
    let peer: std::net::SocketAddr = "192.0.2.91:1".parse().unwrap();
    request.extensions_mut().insert(ConnectInfo(peer));
    let response = router.oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

/// Fresh `iso_accounts` with `accounts`, and no `IsoAccount` audit or
/// outbox rows. The audit and outbox tables are created up front the way
/// migrations would.
pub async fn reset(pool: &PgPool, accounts: &[(i64, i64)]) {
    sqlx::raw_sql(cratestack::AUDIT_TABLE_DDL)
        .execute(pool)
        .await
        .unwrap();
    // `events().drain()` creates the outbox table if it is missing.
    rest::cratestack_schema::Cratestack::builder(pool.clone())
        .build()
        .events()
        .drain()
        .await
        .unwrap();
    sqlx::query("DELETE FROM cratestack_event_outbox WHERE model = 'IsoAccount'")
        .execute(pool)
        .await
        .unwrap();
    for statement in [
        "DROP TABLE IF EXISTS iso_accounts",
        "CREATE TABLE iso_accounts (id BIGINT PRIMARY KEY, balance BIGINT NOT NULL)",
        "DELETE FROM cratestack_audit WHERE model = 'IsoAccount'",
    ] {
        sqlx::query(statement).execute(pool).await.unwrap();
    }
    for (id, balance) in accounts {
        sqlx::query("INSERT INTO iso_accounts (id, balance) VALUES ($1, $2)")
            .bind(id)
            .bind(balance)
            .execute(pool)
            .await
            .unwrap();
    }
}

pub async fn account_balances(pool: &PgPool) -> Vec<(i64, i64)> {
    sqlx::query_as("SELECT id, balance FROM iso_accounts ORDER BY id")
        .fetch_all(pool)
        .await
        .unwrap()
}

pub async fn audit_rows(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*)::BIGINT FROM cratestack_audit WHERE model = 'IsoAccount'")
        .fetch_one(pool)
        .await
        .unwrap()
}
