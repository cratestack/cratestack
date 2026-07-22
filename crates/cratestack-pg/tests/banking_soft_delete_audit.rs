//! Regression test for #117: `delete()` on a model with both
//! `@@soft_delete` and `@@audit` used to write a wrong audit snapshot.
//! Root cause: for a soft-delete model, `delete()` actually runs
//! `UPDATE ... SET deleted_at = NOW() ... RETURNING *`, whose
//! `RETURNING` row is the *post*-update state — but the audit path
//! unconditionally treated that row as `before` and passed `None` for
//! `after`, inverting/losing both snapshots.
//!
//! `@version` makes the divergence directly observable: soft-delete
//! bumps `version` alongside `deleted_at`, so a correct fix must show
//! the pre-bump version in `before` and the post-bump version in
//! `after`.

use cratestack::include_server_schema;
use cratestack::sqlx::{Row, query};
use cratestack::{CoolContext, Value};

include_server_schema!(
    "tests/fixtures/banking_soft_delete_audit.cstack",
    db = Postgres
);

mod support;

use support::pg;

async fn reset_schema(pool: &cratestack::sqlx::PgPool) {
    query("DROP TABLE IF EXISTS cratestack_audit, cratestack_event_outbox, soft_audit_customers")
        .execute(pool)
        .await
        .expect("drop tables");
    query(
        "CREATE TABLE soft_audit_customers (
            id BIGINT PRIMARY KEY,
            name TEXT NOT NULL,
            email TEXT NOT NULL,
            version BIGINT NOT NULL,
            deleted_at TIMESTAMPTZ
        )",
    )
    .execute(pool)
    .await
    .expect("create soft_audit_customers table");
}

fn operator() -> CoolContext {
    CoolContext::authenticated([("id".to_owned(), Value::Int(1))])
}

#[tokio::test]
async fn soft_delete_audit_snapshot_has_correct_before_and_after() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    query(
        "INSERT INTO soft_audit_customers (id, name, email, version) \
         VALUES (1, 'Alice', 'alice@example.com', 1)",
    )
    .execute(pool)
    .await
    .expect("seed");

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    cool.soft_audit_customer()
        .delete(1)
        .run(&ctx)
        .await
        .expect("soft delete succeeds");

    // `cratestack_audit` is a shared table other test binaries write to
    // concurrently — filter by model so a foreign row from another
    // audited test can't be picked up instead of this test's own.
    let rows = query(
        "SELECT operation, before, after FROM cratestack_audit \
         WHERE model = 'SoftAuditCustomer' ORDER BY occurred_at",
    )
    .fetch_all(pool)
    .await
    .expect("fetch audit rows");
    assert_eq!(rows.len(), 1, "expected exactly one audit row");
    let row = &rows[0];

    let op: String = row.get("operation");
    assert_eq!(op, "delete");

    let before: Option<serde_json::Value> = row.get("before");
    let after: Option<serde_json::Value> = row.get("after");

    let before = before.expect("soft-delete audit must have a before snapshot");
    let after = after.expect(
        "soft-delete audit must have an after snapshot — delete() is an UPDATE under the hood, \
         so the post-tombstone state must be captured, not dropped as None",
    );

    // The regression: before used to be the POST-update row (version
    // already bumped) and after was always null. Correct behavior is
    // the reverse — before is pre-mutation, after is post-mutation.
    assert_eq!(
        before["version"],
        serde_json::json!(1),
        "before snapshot must reflect the row's state prior to the soft delete"
    );
    assert_eq!(
        after["version"],
        serde_json::json!(2),
        "after snapshot must reflect the row's state once soft-deleted (version bumped)"
    );
    assert_eq!(before["email"], serde_json::json!("alice@example.com"));
    assert_eq!(after["email"], serde_json::json!("alice@example.com"));
    assert_ne!(
        before, after,
        "before and after must diverge — a soft delete is a real mutation, not a no-op"
    );
}
