//! Policy reads made by a write inside an `@isolation` procedure run on the
//! procedure's own transaction (docs/design/procedure-isolation.md §4.1,
//! GHSA-r67q-4qqq-g9gm): they see the attempt's earlier writes, read its
//! snapshot, and never need a second pooled connection. Every other caller
//! keeps reading policies on the pool — `policy_db_caller_tx.rs` pins that,
//! with the opposite outcome for each case here.
//!
//! Needs a database: `just test-ci-db --test procedure_isolation_policy --
//! --test-threads=1` with `CRATESTACK_REQUIRE_DB=1`.

mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use cratestack::sqlx::{self, PgPool, postgres::PgPoolOptions};
use cratestack::{CratestackContext, CratestackError, Value, include_server_schema};
use support::pg;

include_server_schema!(
    "tests/fixtures/procedure_isolation_policy.cstack",
    db = Postgres
);

use cratestack_schema::procedures as p;
use cratestack_schema::{
    Cratestack, CreateIsoPolAuditedInput, CreateIsoPolFolderInput, CreateIsoPolItemInput,
    CreateIsoPolOwnerInput, Done, IsolatedCratestack, UpdateIsoPolOwnerInput,
};

#[derive(Clone)]
struct Procedures {
    pool: PgPool,
    revoked: Arc<AtomicBool>,
}

fn done(ok: i64, err: i64) -> Done {
    Done { ok, err }
}

impl Procedures {
    async fn revoke_then_create(
        &self,
        db: &IsolatedCratestack,
        ctx: &CratestackContext,
        id: i64,
    ) -> Result<Done, CratestackError> {
        // Take the snapshot, then revoke from another session (once).
        db.transaction(async |tx| {
            sqlx::query("SELECT 1")
                .execute(&mut ***tx)
                .await
                .map(|_| ())
                .map_err(cratestack::cratestack_error_from_sqlx)
        })
        .await?;
        if !self.revoked.swap(true, Ordering::SeqCst) {
            sqlx::query("UPDATE iso_pol_owners SET owner_id = 99 WHERE id = $1")
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(cratestack::cratestack_error_from_sqlx)?;
        }
        db.iso_pol_item()
            .create(CreateIsoPolItemInput { id, ownerRowId: id })
            .run(ctx)
            .await?;
        Ok(done(1, 0))
    }
}

impl p::ProcedureRegistry for Procedures {
    async fn parent_then_child(
        &self,
        db: &IsolatedCratestack,
        ctx: &CratestackContext,
        args: p::parent_then_child::Args,
        _authorized: p::parent_then_child::Authorized,
    ) -> Result<Done, CratestackError> {
        let id = args.args.id;
        db.iso_pol_owner()
            .create(CreateIsoPolOwnerInput { id, ownerId: 1 })
            .run(ctx)
            .await?;
        db.iso_pol_item()
            .create(CreateIsoPolItemInput { id, ownerRowId: id })
            .run(ctx)
            .await?;
        Ok(done(1, 0))
    }

    async fn hand_off_then_child(
        &self,
        db: &IsolatedCratestack,
        ctx: &CratestackContext,
        args: p::hand_off_then_child::Args,
        _authorized: p::hand_off_then_child::Authorized,
    ) -> Result<Done, CratestackError> {
        let id = args.args.id;
        db.iso_pol_owner()
            .update(id)
            .set(UpdateIsoPolOwnerInput { ownerId: Some(99) })
            .run(ctx)
            .await?;
        db.iso_pol_item()
            .create(CreateIsoPolItemInput { id, ownerRowId: id })
            .run(ctx)
            .await?;
        Ok(done(1, 0))
    }

    async fn batch_chain(
        &self,
        db: &IsolatedCratestack,
        ctx: &CratestackContext,
        args: p::batch_chain::Args,
        _authorized: p::batch_chain::Authorized,
    ) -> Result<Done, CratestackError> {
        let id = args.args.id;
        let batch = db
            .iso_pol_folder()
            .batch_create(vec![
                CreateIsoPolFolderInput {
                    id: id + 1,
                    ownerId: 1,
                    parentId: 1,
                },
                CreateIsoPolFolderInput {
                    id: id + 2,
                    ownerId: 1,
                    parentId: id + 1,
                },
            ])
            .run(ctx)
            .await?;
        Ok(done(batch.summary.ok as i64, batch.summary.err as i64))
    }

    async fn revoke_rr(
        &self,
        db: &IsolatedCratestack,
        ctx: &CratestackContext,
        args: p::revoke_rr::Args,
        _authorized: p::revoke_rr::Authorized,
    ) -> Result<Done, CratestackError> {
        self.revoke_then_create(db, ctx, args.args.id).await
    }

    async fn revoke_ser(
        &self,
        db: &IsolatedCratestack,
        ctx: &CratestackContext,
        args: p::revoke_ser::Args,
        _authorized: p::revoke_ser::Authorized,
    ) -> Result<Done, CratestackError> {
        self.revoke_then_create(db, ctx, args.args.id).await
    }

    async fn revoke_rc(
        &self,
        db: &IsolatedCratestack,
        ctx: &CratestackContext,
        args: p::revoke_rc::Args,
        _authorized: p::revoke_rc::Authorized,
    ) -> Result<Done, CratestackError> {
        self.revoke_then_create(db, ctx, args.args.id).await
    }

    async fn audited_child(
        &self,
        db: &IsolatedCratestack,
        ctx: &CratestackContext,
        args: p::audited_child::Args,
        _authorized: p::audited_child::Authorized,
    ) -> Result<Done, CratestackError> {
        let id = args.args.id;
        db.iso_pol_audited()
            .create(CreateIsoPolAuditedInput { id, ownerRowId: id })
            .run(ctx)
            .await?;
        Ok(done(1, 0))
    }
}

fn caller() -> CratestackContext {
    CratestackContext::authenticated([("id".to_owned(), Value::Int(1))])
}

fn registry(pool: &PgPool) -> Procedures {
    Procedures {
        pool: pool.clone(),
        revoked: Arc::new(AtomicBool::new(false)),
    }
}

fn step(id: i64) -> cratestack_schema::Step {
    cratestack_schema::Step { id }
}

/// One call through the generated `invoke_with_db` — the path REST, RPC,
/// `/rpc/batch` and MCP all share.
macro_rules! call {
    ($db:expr, $registry:expr, $proc:ident, $id:expr) => {{
        let args = p::$proc::Args { args: step($id) };
        let (registry, call_args, ctx) = ($registry.clone(), args.clone(), caller());
        p::$proc::invoke_with_db(
            &$db,
            &args,
            &caller(),
            move |tx_db, authorized| async move {
                p::ProcedureRegistry::$proc(&registry, &tx_db, &ctx, call_args, authorized).await
            },
        )
        .await
    }};
}

async fn sql(pool: &PgPool, statement: &str) {
    sqlx::query(sqlx::AssertSqlSafe(statement.to_owned()))
        .execute(pool)
        .await
        .unwrap_or_else(|error| panic!("{statement}: {error}"));
}

async fn reset(pool: &PgPool) {
    sql(
        pool,
        "DROP TABLE IF EXISTS iso_pol_owners, iso_pol_items, iso_pol_auditeds, iso_pol_folders",
    )
    .await;
    for table in [
        "CREATE TABLE iso_pol_owners (id BIGINT PRIMARY KEY, owner_id BIGINT NOT NULL)",
        "CREATE TABLE iso_pol_items (id BIGINT PRIMARY KEY, owner_row_id BIGINT NOT NULL)",
        "CREATE TABLE iso_pol_auditeds (id BIGINT PRIMARY KEY, owner_row_id BIGINT NOT NULL)",
        "CREATE TABLE iso_pol_folders (id BIGINT PRIMARY KEY, owner_id BIGINT NOT NULL, \
         parent_id BIGINT NOT NULL)",
    ] {
        sql(pool, table).await;
    }
}

async fn count(pool: &PgPool, table: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT COUNT(*)::BIGINT FROM {table}"
    )))
    .fetch_one(pool)
    .await
    .expect("count")
}

/// The attempt's own earlier writes are what its policy reads see. On the
/// pool (`policy_db_caller_tx.rs`) each outcome is the opposite.
#[tokio::test]
async fn policy_reads_see_the_attempts_own_writes() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    let db = Cratestack::builder(pool.clone()).build();
    let procedures = registry(pool);

    reset(pool).await;
    let created = call!(db, procedures, parent_then_child, 1);
    assert!(created.is_ok(), "{created:?}");
    assert_eq!(count(pool, "iso_pol_items").await, 1);

    reset(pool).await;
    sql(pool, "INSERT INTO iso_pol_owners VALUES (2, 1)").await;
    let handed_away = call!(db, procedures, hand_off_then_child, 2);
    assert!(
        matches!(handed_away, Err(CratestackError::Forbidden(_))),
        "{handed_away:?}"
    );
    assert_eq!(count(pool, "iso_pol_items").await, 0);
    let owner: i64 = sqlx::query_scalar("SELECT owner_id FROM iso_pol_owners WHERE id = 2")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(owner, 1, "the refused attempt's hand-off was rolled back");

    reset(pool).await;
    sql(pool, "INSERT INTO iso_pol_folders VALUES (1, 1, 1)").await;
    let batch = call!(db, procedures, batch_chain, 10).expect("batch");
    assert_eq!((batch.ok, batch.err), (2, 0));
}

/// Under `REPEATABLE READ` and `SERIALIZABLE` the policy read is on the
/// attempt's snapshot: a revocation committed after it was taken is not
/// seen, and the write commits (a valid serial order places the attempt
/// first). `READ COMMITTED` takes a fresh snapshot per statement.
#[tokio::test]
async fn policy_reads_use_the_attempts_snapshot() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    let db = Cratestack::builder(pool.clone()).build();

    for (level, committed) in [("rr", true), ("ser", true), ("rc", false)] {
        reset(pool).await;
        sql(pool, "INSERT INTO iso_pol_owners VALUES (3, 1)").await;
        let procedures = registry(pool);
        let result = match level {
            "rr" => call!(db, procedures, revoke_rr, 3),
            "ser" => call!(db, procedures, revoke_ser, 3),
            _ => call!(db, procedures, revoke_rc, 3),
        };
        assert_eq!(result.is_ok(), committed, "{level}: {result:?}");
        if !committed {
            assert!(
                matches!(result, Err(CratestackError::Forbidden(_))),
                "{level}: {result:?}"
            );
        }
        assert_eq!(
            count(pool, "iso_pol_items").await,
            i64::from(committed),
            "{level}"
        );
    }
}

/// An audited write whose create policy looks up a relation, on a
/// one-connection pool: the policy read and the audit bootstrap both use the
/// attempt's connection, so nothing waits for a second one. Before
/// cratestack#1117 only an `@isolation` attempt had this; every other caller's
/// transaction is now covered by `policy_db_caller_tx.rs`.
#[tokio::test]
async fn an_audited_write_with_a_relation_policy_needs_one_connection() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    reset(&test_pg.pool).await;
    sqlx::raw_sql(cratestack::AUDIT_TABLE_DDL)
        .execute(&test_pg.pool)
        .await
        .expect("audit table");
    sql(&test_pg.pool, "INSERT INTO iso_pol_owners VALUES (4, 1)").await;
    let one = PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(2))
        .connect(&test_pg.url)
        .await
        .expect("one-connection pool");
    let db = Cratestack::builder(one.clone()).build();
    let procedures = registry(&one);
    let started = Instant::now();
    let result = call!(db, procedures, audited_child, 4);
    let elapsed = started.elapsed();
    assert!(result.is_ok(), "{result:?} after {elapsed:?}");
    assert!(
        elapsed < Duration::from_secs(2),
        "waited for a second connection: {elapsed:?}"
    );
    assert_eq!(count(&test_pg.pool, "iso_pol_auditeds").await, 1);
}

/// Inside an `@isolation` procedure the audit bootstrap asks the attempt
/// whether `cratestack_audit` and its indexes exist before taking a pool
/// connection for the DDL. A table created without those indexes still
/// gets them.
#[tokio::test]
async fn a_hand_created_audit_table_still_gets_its_indexes() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset(pool).await;
    sql(pool, "DROP TABLE IF EXISTS cratestack_audit").await;
    let table_only = cratestack::AUDIT_TABLE_DDL
        .split("CREATE INDEX")
        .next()
        .expect("DDL starts with the table");
    sqlx::raw_sql(sqlx::AssertSqlSafe(table_only.to_owned()))
        .execute(pool)
        .await
        .expect("cratestack_audit without indexes");
    sql(pool, "INSERT INTO iso_pol_owners VALUES (5, 1)").await;

    let db = Cratestack::builder(pool.clone()).build();
    let result = call!(db, registry(pool), audited_child, 5);
    assert!(result.is_ok(), "{result:?}");
    for index in [
        "cratestack_audit_model_idx",
        "cratestack_audit_tenant_idx",
        "cratestack_audit_undelivered_idx",
    ] {
        let exists: bool = sqlx::query_scalar("SELECT to_regclass($1) IS NOT NULL")
            .bind(index)
            .fetch_one(pool)
            .await
            .expect("probe index");
        assert!(exists, "{index} was not created by the audit bootstrap");
    }
}
