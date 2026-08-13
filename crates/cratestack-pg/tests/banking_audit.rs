//! End-to-end test for the `@@audit` model attribute.
//!
//! Spins up a real Postgres, exercises Create/Update/Delete on an
//! audit-enabled model, and asserts that the resulting rows in
//! `cratestack_audit` carry the right operation tag, request id, and
//! redact `@pii` / `@sensitive` columns.

use std::sync::{Arc, Mutex};

use cratestack::include_server_schema;
use cratestack::sqlx::{Row, query};
use cratestack::{
    AuditEvent, AuditSink, BatchItemStatus, CoolContext, CoolError, UpsertOutcome, Value,
};

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

/// cratestack#473 review finding: the two tests above only exercise the
/// thin `run()` wrapper around a single-item write. `batch_create` is
/// structurally different — it collects `AuditEvent`s across a
/// per-item savepoint loop and dispatches them once, after the *outer*
/// transaction commits (see `cratestack_sqlx::query::batch::create`) —
/// so it needs its own coverage rather than relying on the single-item
/// path to stand in for it. This also proves that a per-item failure
/// (caught by its own savepoint, not the outer transaction) does not
/// reach the sink, mirroring the single-item rollback test above.
#[tokio::test]
async fn custom_audit_sink_receives_one_event_per_successful_batch_create_item() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    // Seed id 20 so the third batch item collides on the primary key and
    // fails at the per-item savepoint without rolling back the other two.
    query("INSERT INTO accounts VALUES (20, 'zack@example.com', 1, 1)")
        .execute(pool)
        .await
        .expect("seed");

    let sink = RecordingAuditSink::default();
    let cool = cratestack_schema::Cratestack::builder(pool.clone())
        .with_audit_sink(std::sync::Arc::new(sink.clone()))
        .build();
    let ctx = operator();

    let response = cool
        .account()
        .batch_create(vec![
            cratestack_schema::CreateAccountInput {
                id: 21,
                customerEmail: "wendy@example.com".to_owned(),
                riskScore: 5,
                balance: 1_000,
            },
            cratestack_schema::CreateAccountInput {
                id: 22,
                customerEmail: "victor@example.com".to_owned(),
                riskScore: 6,
                balance: 2_000,
            },
            cratestack_schema::CreateAccountInput {
                id: 20, // duplicate primary key -> per-item savepoint rollback
                customerEmail: "zack@example.com".to_owned(),
                riskScore: 7,
                balance: 3_000,
            },
        ])
        .run(&ctx)
        .await
        .expect("batch_create infra ok despite one failing item");

    assert_eq!(response.summary.ok, 2, "two items should succeed");
    assert_eq!(
        response.summary.err, 1,
        "one item should fail on PK conflict"
    );
    assert!(
        matches!(response.results[2].status, BatchItemStatus::Error { .. }),
        "the third item (duplicate id 20) must report an error status"
    );

    // The sink must observe exactly one event per *successful* item —
    // not three, and not zero.
    let recorded = sink.events.lock().unwrap().clone();
    assert_eq!(
        recorded.len(),
        2,
        "custom AuditSink should receive exactly one event per successful batch_create item, \
         not the failed one"
    );
    let mut recorded_ids: Vec<serde_json::Value> = recorded
        .iter()
        .map(|event| event.primary_key.clone())
        .collect();
    recorded_ids.sort_by_key(|v| v.to_string());
    assert_eq!(
        recorded_ids,
        vec![serde_json::json!(21), serde_json::json!(22)],
        "the sink should have observed events for ids 21 and 22, not the failed id 20"
    );

    // The DB audit table must agree with the sink: two rows, matching
    // the two successful items, alongside the pre-existing seed row.
    let audit_count: i64 = query(
        "SELECT COUNT(*)::BIGINT FROM cratestack_audit \
         WHERE model = 'Account' AND operation = 'create'",
    )
    .fetch_one(pool)
    .await
    .expect("count audit rows")
    .get(0);
    assert_eq!(
        audit_count, 2,
        "the in-database audit table must record the same two successful creates as the sink"
    );
}

// ───── cratestack#473 coverage-gap closure ───────────────────────────────
//
// `dispatch_audit_sink` is wired into 11 structurally identical call
// sites (7 under `cratestack_sqlx::query::write`, 4 under
// `cratestack_sqlx::query::batch`), but before the tests below only
// `create` and `batch_create` were ever asserted against an installed
// `AuditSink` — the other nine were unverified copy/paste. Each test
// below targets exactly one of the remaining sites; see the PR
// description for the sabotage-run proof that each assertion is bound
// to its own call site (commenting out that site's `dispatch_audit_sink`
// call turns exactly that test red and no other).
//
//   write/create.rs            -> covered above (`custom_audit_sink_receives_the_create_event`)
//   write/delete.rs             -> `custom_audit_sink_receives_the_delete_event`
//   write/update_run.rs         -> `custom_audit_sink_receives_the_update_event`
//   write/upsert.rs              -> `custom_audit_sink_receives_both_branches_of_plain_upsert`
//   write/upsert_do_nothing.rs  -> `custom_audit_sink_receives_the_upsert_do_nothing_insert_event_only`
//   write/update_many.rs        -> `custom_audit_sink_receives_the_update_many_event`
//   write/delete_many.rs        -> `custom_audit_sink_receives_the_delete_many_event`
//   batch/create.rs              -> covered above (`custom_audit_sink_receives_one_event_per_successful_batch_create_item`)
//   batch/update.rs              -> `custom_audit_sink_receives_one_event_per_successful_batch_update_item`
//   batch/delete.rs              -> `custom_audit_sink_receives_one_event_per_successful_batch_delete_item`
//   batch/upsert.rs              -> `custom_audit_sink_receives_both_branches_of_batch_upsert`

fn install_sink(
    pool: &cratestack::sqlx::PgPool,
) -> (RecordingAuditSink, cratestack_schema::Cratestack) {
    let sink = RecordingAuditSink::default();
    let cool = cratestack_schema::Cratestack::builder(pool.clone())
        .with_audit_sink(std::sync::Arc::new(sink.clone()))
        .build();
    (sink, cool)
}

/// `write/update_run.rs`'s `dispatch_audit_sink` call, driven by the
/// single-row `.update(id).set(..).run(ctx)` path.
#[tokio::test]
async fn custom_audit_sink_receives_the_update_event() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    query("INSERT INTO accounts VALUES (30, 'greta@example.com', 1, 1)")
        .execute(pool)
        .await
        .expect("seed");

    let (sink, cool) = install_sink(pool);
    let ctx = operator();

    cool.account()
        .update(30)
        .set(cratestack_schema::UpdateAccountInput {
            customerEmail: None,
            riskScore: None,
            balance: Some(500),
        })
        .run(&ctx)
        .await
        .expect("update succeeds");

    let recorded = sink.events.lock().unwrap().clone();
    assert_eq!(
        recorded.len(),
        1,
        "the AuditSink must receive exactly one event for the single-row update path"
    );
    assert_eq!(recorded[0].operation.as_str(), "update");
    assert_eq!(recorded[0].primary_key, serde_json::json!(30));
}

/// `write/delete.rs`'s `dispatch_audit_sink` call, driven by the
/// single-row `.delete(id).run(ctx)` path.
#[tokio::test]
async fn custom_audit_sink_receives_the_delete_event() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    query("INSERT INTO accounts VALUES (31, 'hank@example.com', 1, 1)")
        .execute(pool)
        .await
        .expect("seed");

    let (sink, cool) = install_sink(pool);
    let ctx = operator();

    cool.account()
        .delete(31)
        .run(&ctx)
        .await
        .expect("delete succeeds");

    let recorded = sink.events.lock().unwrap().clone();
    assert_eq!(
        recorded.len(),
        1,
        "the AuditSink must receive exactly one event for the single-row delete path"
    );
    assert_eq!(recorded[0].operation.as_str(), "delete");
    assert_eq!(recorded[0].primary_key, serde_json::json!(31));
}

/// `write/upsert.rs`'s single `dispatch_audit_sink` call site, exercised
/// across both branches it can fire from (insert then DO UPDATE).
#[tokio::test]
async fn custom_audit_sink_receives_both_branches_of_plain_upsert() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    let (sink, cool) = install_sink(pool);
    let ctx = operator();

    // Insert branch: no existing row for id 32.
    cool.account()
        .upsert(cratestack_schema::CreateAccountInput {
            id: 32,
            customerEmail: "ida@example.com".to_owned(),
            riskScore: 1,
            balance: 100,
        })
        .run(&ctx)
        .await
        .expect("upsert insert branch succeeds");

    // DO UPDATE branch: same id, conflicts with the row just inserted.
    cool.account()
        .upsert(cratestack_schema::CreateAccountInput {
            id: 32,
            customerEmail: "ida@example.com".to_owned(),
            riskScore: 1,
            balance: 200,
        })
        .run(&ctx)
        .await
        .expect("upsert update branch succeeds");

    let recorded = sink.events.lock().unwrap().clone();
    assert_eq!(
        recorded.len(),
        2,
        "the AuditSink must receive one event per plain-upsert call, across both branches"
    );
    assert_eq!(recorded[0].operation.as_str(), "create");
    assert_eq!(recorded[1].operation.as_str(), "update");
    assert_eq!(recorded[0].primary_key, serde_json::json!(32));
    assert_eq!(recorded[1].primary_key, serde_json::json!(32));
}

/// `write/upsert_do_nothing.rs`'s `dispatch_audit_sink` call site — a
/// distinct call site from plain `.upsert().run()` above. Only the
/// insert branch ever builds an `AuditEvent`; the DO NOTHING conflict
/// branch never mutates the row, so it must not reach the sink either.
#[tokio::test]
async fn custom_audit_sink_receives_the_upsert_do_nothing_insert_event_only() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    let (sink, cool) = install_sink(pool);
    let ctx = operator();

    let first = cool
        .account()
        .upsert(cratestack_schema::CreateAccountInput {
            id: 33,
            customerEmail: "jack@example.com".to_owned(),
            riskScore: 1,
            balance: 100,
        })
        .do_nothing()
        .run(&ctx)
        .await
        .expect("do_nothing insert branch succeeds");
    assert!(matches!(first, UpsertOutcome::Inserted(_)));

    let second = cool
        .account()
        .upsert(cratestack_schema::CreateAccountInput {
            id: 33,
            customerEmail: "jack@example.com".to_owned(),
            riskScore: 1,
            balance: 999, // ignored: DO NOTHING must not touch the row
        })
        .do_nothing()
        .run(&ctx)
        .await
        .expect("do_nothing existing branch succeeds");
    assert!(matches!(second, UpsertOutcome::Existing(_)));

    let recorded = sink.events.lock().unwrap().clone();
    assert_eq!(
        recorded.len(),
        1,
        "the AuditSink must receive exactly one event — the insert branch — \
         since DO NOTHING's existing branch never writes an audit row"
    );
    assert_eq!(recorded[0].operation.as_str(), "create");
    assert_eq!(recorded[0].primary_key, serde_json::json!(33));
}

/// `write/update_many.rs`'s `dispatch_audit_sink` call, driven by the
/// predicate-scoped `.update_many().where_(..).set(..).run(ctx)` path.
#[tokio::test]
async fn custom_audit_sink_receives_the_update_many_event() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    query("INSERT INTO accounts VALUES (34, 'ken@example.com', 1, 1)")
        .execute(pool)
        .await
        .expect("seed");

    let (sink, cool) = install_sink(pool);
    let ctx = operator();

    use cratestack_schema::account;
    let summary = cool
        .account()
        .update_many()
        .where_(account::id().eq(34))
        .set(cratestack_schema::UpdateAccountInput {
            customerEmail: None,
            riskScore: None,
            balance: Some(700),
        })
        .run(&ctx)
        .await
        .expect("update_many succeeds");
    assert_eq!(summary.ok, 1);

    let recorded = sink.events.lock().unwrap().clone();
    assert_eq!(
        recorded.len(),
        1,
        "the AuditSink must receive exactly one event for the one row update_many touched"
    );
    assert_eq!(recorded[0].operation.as_str(), "update");
    assert_eq!(recorded[0].primary_key, serde_json::json!(34));
}

/// `write/delete_many.rs`'s `dispatch_audit_sink` call, driven by the
/// predicate-scoped `.delete_many().where_(..).run(ctx)` path.
#[tokio::test]
async fn custom_audit_sink_receives_the_delete_many_event() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    query("INSERT INTO accounts VALUES (35, 'liz@example.com', 1, 1)")
        .execute(pool)
        .await
        .expect("seed");

    let (sink, cool) = install_sink(pool);
    let ctx = operator();

    use cratestack_schema::account;
    let summary = cool
        .account()
        .delete_many()
        .where_(account::id().eq(35))
        .run(&ctx)
        .await
        .expect("delete_many succeeds");
    assert_eq!(summary.ok, 1);

    let recorded = sink.events.lock().unwrap().clone();
    assert_eq!(
        recorded.len(),
        1,
        "the AuditSink must receive exactly one event for the one row delete_many touched"
    );
    assert_eq!(recorded[0].operation.as_str(), "delete");
    assert_eq!(recorded[0].primary_key, serde_json::json!(35));
}

/// `batch/update.rs`'s `dispatch_audit_sink` call — a distinct call site
/// from the single-row `update_run.rs` one above, dispatched once for
/// the whole batch after the outer transaction commits.
#[tokio::test]
async fn custom_audit_sink_receives_one_event_per_successful_batch_update_item() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    query(
        "INSERT INTO accounts VALUES (36, 'mia@example.com', 1, 1), \
         (37, 'noah@example.com', 1, 1)",
    )
    .execute(pool)
    .await
    .expect("seed");

    let (sink, cool) = install_sink(pool);
    let ctx = operator();

    let response = cool
        .account()
        .batch_update(vec![
            (
                36,
                cratestack_schema::UpdateAccountInput {
                    customerEmail: None,
                    riskScore: None,
                    balance: Some(360),
                },
                None,
            ),
            (
                37,
                cratestack_schema::UpdateAccountInput {
                    customerEmail: None,
                    riskScore: None,
                    balance: Some(370),
                },
                None,
            ),
        ])
        .run(&ctx)
        .await
        .expect("batch_update infra ok");
    assert_eq!(response.summary.ok, 2);

    let recorded = sink.events.lock().unwrap().clone();
    assert_eq!(
        recorded.len(),
        2,
        "the AuditSink must receive one event per successful batch_update item"
    );
    let mut ids: Vec<serde_json::Value> = recorded
        .iter()
        .map(|event| event.primary_key.clone())
        .collect();
    ids.sort_by_key(|v| v.to_string());
    assert_eq!(ids, vec![serde_json::json!(36), serde_json::json!(37)]);
    assert!(
        recorded
            .iter()
            .all(|event| event.operation.as_str() == "update")
    );
}

/// `batch/delete.rs`'s `dispatch_audit_sink` call — a distinct call site
/// from the single-row `delete.rs` one above.
#[tokio::test]
async fn custom_audit_sink_receives_one_event_per_successful_batch_delete_item() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    query(
        "INSERT INTO accounts VALUES (38, 'omar@example.com', 1, 1), \
         (39, 'paula@example.com', 1, 1)",
    )
    .execute(pool)
    .await
    .expect("seed");

    let (sink, cool) = install_sink(pool);
    let ctx = operator();

    let response = cool
        .account()
        .batch_delete(vec![38, 39, 9999]) // 9999 doesn't exist -> per-item NotFound
        .run(&ctx)
        .await
        .expect("batch_delete infra ok");
    assert_eq!(response.summary.ok, 2);
    assert_eq!(response.summary.err, 1);

    let recorded = sink.events.lock().unwrap().clone();
    assert_eq!(
        recorded.len(),
        2,
        "the AuditSink must receive one event per successful batch_delete item, \
         not the NotFound one"
    );
    let mut ids: Vec<serde_json::Value> = recorded
        .iter()
        .map(|event| event.primary_key.clone())
        .collect();
    ids.sort_by_key(|v| v.to_string());
    assert_eq!(ids, vec![serde_json::json!(38), serde_json::json!(39)]);
    assert!(
        recorded
            .iter()
            .all(|event| event.operation.as_str() == "delete")
    );
}

/// `batch/upsert.rs`'s single `dispatch_audit_sink` call site — a
/// distinct call site from the single-row `upsert.rs` one above,
/// exercised across both branches it can fire from within one batch.
#[tokio::test]
async fn custom_audit_sink_receives_both_branches_of_batch_upsert() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    // Pre-existing row for id 40 so batch_upsert's matching item hits
    // the UPDATE branch, while id 41 is new (INSERT branch).
    query("INSERT INTO accounts VALUES (40, 'quinn@example.com', 1, 1)")
        .execute(pool)
        .await
        .expect("seed");

    let (sink, cool) = install_sink(pool);
    let ctx = operator();

    let response = cool
        .account()
        .batch_upsert(vec![
            cratestack_schema::CreateAccountInput {
                id: 41,
                customerEmail: "riley@example.com".to_owned(),
                riskScore: 2,
                balance: 41,
            },
            cratestack_schema::CreateAccountInput {
                id: 40,
                customerEmail: "quinn@example.com".to_owned(),
                riskScore: 2,
                balance: 40,
            },
        ])
        .run(&ctx)
        .await
        .expect("batch_upsert infra ok");
    assert_eq!(response.summary.ok, 2);

    let recorded = sink.events.lock().unwrap().clone();
    assert_eq!(
        recorded.len(),
        2,
        "the AuditSink must receive one event per batch_upsert item, across both branches"
    );
    let ops_by_pk: std::collections::BTreeMap<String, String> = recorded
        .iter()
        .map(|event| {
            (
                event.primary_key.to_string(),
                event.operation.as_str().to_owned(),
            )
        })
        .collect();
    assert_eq!(ops_by_pk.get("41").map(String::as_str), Some("create"));
    assert_eq!(ops_by_pk.get("40").map(String::as_str), Some("update"));
}
