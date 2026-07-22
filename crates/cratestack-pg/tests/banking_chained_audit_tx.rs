//! Regression test for #116: two `run_in_tx` calls to `@@audit`-enabled
//! models chained inside a single caller-managed transaction used to
//! self-deadlock permanently. Root cause was `ensure_audit_table`
//! unconditionally re-issuing `CREATE INDEX IF NOT EXISTS` on every
//! call — even the second one within the same open transaction — which
//! takes a `ShareLock` that conflicts with the `RowExclusiveLock` the
//! first call's own audit insert is already holding.
//!
//! `ensure_audit_table` now caches "ensured" per `SqlxRuntime`, so the
//! second call in the same transaction skips the DDL (and the lock)
//! entirely. This test fails by hanging forever if that regresses, so
//! the chained writes run under a bounded `tokio::time::timeout`
//! instead of relying on the test harness's own timeout to surface it.

use std::time::Duration;

use cratestack::include_server_schema;
use cratestack::sqlx::{Row, query};
use cratestack::{CoolContext, Value};

include_server_schema!(
    "tests/fixtures/banking_chained_audit_tx.cstack",
    db = Postgres
);

mod support;

use support::pg;

async fn reset_schema(pool: &cratestack::sqlx::PgPool) {
    query("DROP TABLE IF EXISTS cratestack_audit, cratestack_event_outbox, audit_rotation_keys")
        .execute(pool)
        .await
        .expect("drop tables");
    query(
        "CREATE TABLE audit_rotation_keys (
            id BIGINT PRIMARY KEY,
            label TEXT NOT NULL,
            revoked BOOLEAN NOT NULL
        )",
    )
    .execute(pool)
    .await
    .expect("create audit_rotation_keys table");
}

fn operator() -> CoolContext {
    CoolContext::authenticated([("id".to_owned(), Value::Int(1))])
}

#[tokio::test]
async fn chained_run_in_tx_audited_writes_do_not_deadlock() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    query("INSERT INTO audit_rotation_keys (id, label, revoked) VALUES (1, 'old-key', false)")
        .execute(pool)
        .await
        .expect("seed");

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    // Mirrors the reported repro: an atomic two-write rotate — revoke
    // the old row, then insert the replacement — both audited, both in
    // one caller-managed transaction.
    let outcome = tokio::time::timeout(Duration::from_secs(15), async {
        let mut tx = pool.begin().await.expect("begin caller-managed tx");

        cool.audit_rotation_key()
            .update(1)
            .set(cratestack_schema::UpdateAuditRotationKeyInput {
                label: None,
                revoked: Some(true),
            })
            .run_in_tx(&mut tx, &ctx)
            .await
            .expect("first audited write in tx (revoke)");

        cool.audit_rotation_key()
            .create(cratestack_schema::CreateAuditRotationKeyInput {
                id: 2,
                label: "new-key".to_owned(),
                revoked: false,
            })
            .run_in_tx(&mut tx, &ctx)
            .await
            .expect("second audited write in tx (create) — this is the call that used to hang");

        tx.commit().await.expect("commit");
    })
    .await;

    assert!(
        outcome.is_ok(),
        "chained run_in_tx audited writes self-deadlocked inside the caller-managed transaction \
         (or otherwise took >15s) — ensure_audit_table's per-runtime cache regressed",
    );

    // `cratestack_audit` is a shared table other test binaries write to
    // concurrently — filter by model so this count can't be inflated
    // (or coincidentally satisfied) by unrelated rows from another test.
    let audit_rows: i64 =
        query("SELECT COUNT(*)::BIGINT FROM cratestack_audit WHERE model = 'AuditRotationKey'")
            .fetch_one(pool)
            .await
            .expect("count audit rows")
            .get(0);
    assert_eq!(
        audit_rows, 2,
        "both chained audited writes should have committed their audit rows",
    );
}
