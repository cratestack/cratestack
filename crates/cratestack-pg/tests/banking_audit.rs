//! End-to-end test for the `@@audit` model attribute.
//!
//! Spins up a real Postgres, exercises Create/Update/Delete on an
//! audit-enabled model, and asserts that the resulting rows in
//! `cratestack_audit` carry the right operation tag, request id, and
//! redact `@pii` / `@sensitive` columns.

use std::sync::{Arc, Mutex};

use cratestack::include_server_schema;
use cratestack::sqlx::{Row, query};
use cratestack::{AuditEvent, AuditSink, CoolContext, CoolError, Value};

include_server_schema!("tests/fixtures/banking_audit.cstack", db = Postgres);

mod support;

use support::pg;

/// Test double for cratestack#473: records every event handed to it so
/// tests can assert the `AuditSink` installation path is actually
/// reachable from a mutation, not just constructible.
#[derive(Clone, Default)]
struct RecordingAuditSink {
    events: Arc<Mutex<Vec<AuditEvent>>>,
}

#[async_trait::async_trait]
impl AuditSink for RecordingAuditSink {
    async fn record(&self, event: &AuditEvent) -> Result<(), CoolError> {
        self.events.lock().unwrap().push(event.clone());
        Ok(())
    }
}

async fn reset_schema(pool: &cratestack::sqlx::PgPool) {
    query("DROP TABLE IF EXISTS cratestack_audit, cratestack_event_outbox, accounts")
        .execute(pool)
        .await
        .expect("drop tables");
    query(
        "CREATE TABLE accounts (
            id BIGINT PRIMARY KEY,
            customer_email TEXT NOT NULL,
            risk_score BIGINT NOT NULL,
            balance BIGINT NOT NULL
        )",
    )
    .execute(pool)
    .await
    .expect("create account table");
}

fn operator() -> CoolContext {
    CoolContext::authenticated([
        ("id".to_owned(), Value::String("operator-7".to_owned())),
        ("role".to_owned(), Value::String("admin".to_owned())),
    ])
    .with_request_id("audit-trace-id-001")
    .with_client_ip("203.0.113.7")
}

#[tokio::test]
async fn audit_captures_create_with_redacted_pii() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    let created = cool
        .account()
        .create(cratestack_schema::CreateAccountInput {
            id: 1,
            customerEmail: "alice@example.com".to_owned(),
            riskScore: 87,
            balance: 100_000,
        })
        .run(&ctx)
        .await
        .expect("create succeeds");
    assert_eq!(created.id, 1);

    // Single audit row should exist.
    let rows = query(
        "SELECT model, operation, primary_key, actor, tenant, before, after, request_id \
         FROM cratestack_audit ORDER BY occurred_at",
    )
    .fetch_all(pool)
    .await
    .expect("fetch audit rows");
    assert_eq!(rows.len(), 1, "expected exactly one audit row");
    let row = &rows[0];

    let model: String = row.get("model");
    assert_eq!(model, "Account");
    let op: String = row.get("operation");
    assert_eq!(op, "create");

    // Primary key should be present and equal to 1 (matches the model's @id).
    let pk: serde_json::Value = row.get("primary_key");
    assert!(
        pk == serde_json::json!(1) || pk == serde_json::json!("1"),
        "primary_key should record the row id, got {pk}",
    );

    // Actor block captures the operator id from the auth context.
    let actor: serde_json::Value = row.get("actor");
    assert_eq!(
        actor["id"],
        serde_json::json!("operator-7"),
        "actor.id should mirror ctx.principal_actor_id",
    );
    assert_eq!(
        actor["ip"],
        serde_json::json!("203.0.113.7"),
        "actor.ip should mirror ctx.client_ip",
    );

    // No tenant attached in this test context.
    let tenant: Option<String> = row.get("tenant");
    assert!(tenant.is_none());

    // Request id round-trips W3C-style.
    let request_id: Option<String> = row.get("request_id");
    assert_eq!(request_id.as_deref(), Some("audit-trace-id-001"));

    // before should be NULL for a create.
    let before: Option<serde_json::Value> = row.get("before");
    assert!(
        before.is_none(),
        "create audit should not have a before snapshot"
    );

    // after should be present, but `customer_email` must be redacted because
    // the field declares `@pii`.
    let after: serde_json::Value = row.get("after");
    assert_eq!(after["customerEmail"], serde_json::json!("[redacted-pii]"));
    assert_eq!(
        after["riskScore"],
        serde_json::json!("[redacted-sensitive]")
    );
    // Non-classified columns survive verbatim.
    assert_eq!(after["balance"], serde_json::json!(100_000));
}

#[tokio::test]
async fn audit_captures_update_before_and_after_with_redaction() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    // Seed directly so the audit-capture path runs on the update, not the create.
    query("INSERT INTO accounts VALUES (2, 'bob@example.com', 50, 250)")
        .execute(pool)
        .await
        .expect("seed");

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    cool.account()
        .update(2)
        .set(cratestack_schema::UpdateAccountInput {
            customerEmail: None,
            riskScore: Some(99),
            balance: Some(1_000),
        })
        .run(&ctx)
        .await
        .expect("update succeeds");

    let rows = query("SELECT operation, before, after FROM cratestack_audit ORDER BY occurred_at")
        .fetch_all(pool)
        .await
        .expect("fetch audit rows");
    assert_eq!(rows.len(), 1);
    let row = &rows[0];

    let op: String = row.get("operation");
    assert_eq!(op, "update");

    let before: serde_json::Value = row.get("before");
    let after: serde_json::Value = row.get("after");

    // Both snapshots must carry the redacted markers for classified columns.
    assert_eq!(before["customerEmail"], serde_json::json!("[redacted-pii]"));
    assert_eq!(
        before["riskScore"],
        serde_json::json!("[redacted-sensitive]")
    );
    assert_eq!(after["customerEmail"], serde_json::json!("[redacted-pii]"));
    assert_eq!(
        after["riskScore"],
        serde_json::json!("[redacted-sensitive]")
    );

    // Non-classified columns must reflect the actual mutation.
    assert_eq!(before["balance"], serde_json::json!(250));
    assert_eq!(after["balance"], serde_json::json!(1_000));
}

#[tokio::test]
async fn audit_captures_delete_with_before_snapshot_and_no_after() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    query("INSERT INTO accounts VALUES (3, 'carol@example.com', 10, 500)")
        .execute(pool)
        .await
        .expect("seed");

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    cool.account()
        .delete(3)
        .run(&ctx)
        .await
        .expect("delete succeeds");

    let rows = query("SELECT operation, before, after FROM cratestack_audit")
        .fetch_all(pool)
        .await
        .expect("fetch audit rows");
    assert_eq!(rows.len(), 1);
    let row = &rows[0];

    let op: String = row.get("operation");
    assert_eq!(op, "delete");

    let before: serde_json::Value = row.get("before");
    let after: Option<serde_json::Value> = row.get("after");
    assert!(
        after.is_none(),
        "delete audit must not carry an after snapshot"
    );
    // Before is fully populated with the pre-delete state (redacted).
    assert_eq!(before["balance"], serde_json::json!(500));
    assert_eq!(before["customerEmail"], serde_json::json!("[redacted-pii]"));
}

#[tokio::test]
async fn audit_row_lives_inside_the_same_transaction_as_the_mutation() {
    // If the audit insert ever escaped the mutation tx, a failing create
    // (e.g. constraint violation) could leave an orphan audit row. We force
    // a duplicate-key create and assert that no audit row appears.
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    query("INSERT INTO accounts VALUES (4, 'dave@example.com', 1, 1)")
        .execute(pool)
        .await
        .expect("seed");

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    let result = cool
        .account()
        .create(cratestack_schema::CreateAccountInput {
            id: 4, // duplicate primary key
            customerEmail: "dave@example.com".to_owned(),
            riskScore: 1,
            balance: 1,
        })
        .run(&ctx)
        .await;
    assert!(result.is_err(), "duplicate-key create must fail");

    let row_count: i64 = query("SELECT COUNT(*)::BIGINT FROM cratestack_audit")
        .fetch_one(pool)
        .await
        .expect("count audit")
        .get(0);
    assert_eq!(
        row_count, 0,
        "no audit row should be persisted when the mutation rolls back",
    );
}

/// cratestack#473: `AuditSink` used to be a dead extension point — no
/// installation path, `record()` never invoked. This asserts a
/// consumer-supplied sink installed via `CratestackBuilder::with_audit_sink`
/// actually receives the event a `@@audit` mutation produces, alongside
/// (not instead of) the `cratestack_audit` table row.
#[tokio::test]
async fn custom_audit_sink_receives_the_create_event() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    let sink = RecordingAuditSink::default();
    let cool = cratestack_schema::Cratestack::builder(pool.clone())
        .with_audit_sink(std::sync::Arc::new(sink.clone()))
        .build();
    let ctx = operator();

    let created = cool
        .account()
        .create(cratestack_schema::CreateAccountInput {
            id: 5,
            customerEmail: "erin@example.com".to_owned(),
            riskScore: 42,
            balance: 7_500,
        })
        .run(&ctx)
        .await
        .expect("create succeeds");
    assert_eq!(created.id, 5);

    // The sink must have observed exactly one event, matching the
    // mutation that just ran.
    let recorded = sink.events.lock().unwrap().clone();
    assert_eq!(
        recorded.len(),
        1,
        "custom AuditSink should receive exactly one event for one create"
    );
    assert_eq!(recorded[0].model, "Account");
    assert_eq!(recorded[0].operation.as_str(), "create");
    assert_eq!(
        recorded[0].actor.id.as_deref(),
        Some("operator-7"),
        "the sink's event should carry the same actor as the DB audit row"
    );

    // The DB row is still the source of truth and must exist too — the
    // sink is an addition, not a replacement.
    let db_row_count: i64 = query("SELECT COUNT(*)::BIGINT FROM cratestack_audit")
        .fetch_one(pool)
        .await
        .expect("count audit")
        .get(0);
    assert_eq!(
        db_row_count, 1,
        "the in-database audit row must still be written even with a sink installed"
    );
}

/// A rolled-back mutation must not reach the sink either — mirrors
/// `audit_row_lives_inside_the_same_transaction_as_the_mutation` for
/// the DB row, but for the `AuditSink` fan-out path: since dispatch
/// only runs after `tx.commit()` succeeds, a failed create must leave
/// the sink untouched.
#[tokio::test]
async fn custom_audit_sink_does_not_receive_events_for_a_rolled_back_mutation() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    query("INSERT INTO accounts VALUES (6, 'frank@example.com', 1, 1)")
        .execute(pool)
        .await
        .expect("seed");

    let sink = RecordingAuditSink::default();
    let cool = cratestack_schema::Cratestack::builder(pool.clone())
        .with_audit_sink(std::sync::Arc::new(sink.clone()))
        .build();
    let ctx = operator();

    let result = cool
        .account()
        .create(cratestack_schema::CreateAccountInput {
            id: 6, // duplicate primary key -> rolls back
            customerEmail: "frank@example.com".to_owned(),
            riskScore: 1,
            balance: 1,
        })
        .run(&ctx)
        .await;
    assert!(result.is_err(), "duplicate-key create must fail");

    assert!(
        sink.events.lock().unwrap().is_empty(),
        "a rolled-back mutation must never reach the installed AuditSink"
    );
}
