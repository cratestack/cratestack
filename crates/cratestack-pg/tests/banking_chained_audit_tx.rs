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
//!
//! Also covers cratestack#534: the exact shape above used to produce
//! ZERO `AuditSink` events and ZERO delivered `@@emit` events for a real
//! installed sink/subscriber, silently, even though the
//! `cratestack_audit` rows (and the outbox rows) committed correctly.
//! `chained_run_in_tx_writes_fan_out_to_installed_sink_after_caller_commits`
//! and `chained_run_in_tx_outbox_events_are_delivered_only_after_explicit_drain`
//! below are the decisive tests for that fix.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use cratestack::include_server_schema;
use cratestack::sqlx::{Row, query};
use cratestack::{AuditEvent, AuditSink, CoolContext, CoolError, Value};

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

/// Test double mirroring `banking_audit.rs`'s `RecordingAuditSink`
/// (cratestack#473) — records every event handed to it so this file can
/// assert the sink is actually reachable for `run_in_tx`-composed
/// writes, not just `run()` ones.
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

/// **The decisive test for cratestack#534.** Before this fix, an
/// installed `AuditSink` observed nothing at all for this exact shape —
/// two `run_in_tx` writes chained in one caller-managed transaction —
/// even though `cratestack_audit` got both rows (proven above). Now
/// `run_in_tx` hands back the `AuditEvent`(s) it built via
/// `RunInTxOutcome`, and `Cratestack::dispatch_audit_sink` is the public
/// opt-in the caller invokes once, after their own commit succeeds.
#[tokio::test]
async fn chained_run_in_tx_writes_fan_out_to_installed_sink_after_caller_commits() {
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

    let sink = RecordingAuditSink::default();
    let cool = cratestack_schema::Cratestack::builder(pool.clone())
        .with_audit_sink(Arc::new(sink.clone()))
        .build();
    let ctx = operator();

    let mut tx = pool.begin().await.expect("begin caller-managed tx");
    let mut audit_events = Vec::new();

    let revoke = cool
        .audit_rotation_key()
        .update(1)
        .set(cratestack_schema::UpdateAuditRotationKeyInput {
            label: None,
            revoked: Some(true),
        })
        .run_in_tx(&mut tx, &ctx)
        .await
        .expect("first audited write in tx (revoke)");
    audit_events.extend(revoke.audit_events);

    let create = cool
        .audit_rotation_key()
        .create(cratestack_schema::CreateAuditRotationKeyInput {
            id: 2,
            label: "new-key".to_owned(),
            revoked: false,
        })
        .run_in_tx(&mut tx, &ctx)
        .await
        .expect("second audited write in tx (create)");
    audit_events.extend(create.audit_events);

    assert_eq!(
        audit_events.len(),
        2,
        "run_in_tx should hand back one AuditEvent per @@audit write, for the caller to \
         dispatch — one from the update, one from the create",
    );
    assert!(
        sink.events.lock().unwrap().is_empty(),
        "the sink must not observe anything before tx.commit() — dispatch never runs from \
         inside a transaction",
    );

    tx.commit().await.expect("commit");

    // Dispatch is caller-driven, not automatic: committing alone must
    // not have caused anything to reach the sink yet.
    assert!(
        sink.events.lock().unwrap().is_empty(),
        "the sink must still observe nothing until dispatch_audit_sink is called explicitly — \
         run_in_tx has no way to do this for the caller",
    );

    cool.dispatch_audit_sink(&audit_events).await;

    // *** This assertion used to be unreachable: before cratestack#534,
    // nothing in this crate could ever make a real installed AuditSink
    // observe events for a run_in_tx-composed transaction. ***
    let recorded = sink.events.lock().unwrap().clone();
    assert_eq!(
        recorded.len(),
        2,
        "the installed AuditSink should observe BOTH run_in_tx writes once the caller \
         dispatches after their own commit succeeds",
    );
    let mut operations: Vec<&str> = recorded.iter().map(|e| e.operation.as_str()).collect();
    operations.sort_unstable();
    assert_eq!(operations, vec!["create", "update"]);
}

/// The `@@emit` half of cratestack#534. Unlike the `AuditSink` fix
/// above, this needed no new API: `Cratestack::events().drain()` already
/// existed (cratestack#390) and re-scans `cratestack_event_outbox` for
/// anything not yet marked delivered, rather than needing a specific
/// event handed back from `run_in_tx` — so it was already a working
/// caller-driven opt-in for exactly this gap. Nothing exercised that
/// combination before, though, so this proves it actually closes it.
#[tokio::test]
async fn chained_run_in_tx_outbox_events_are_delivered_only_after_explicit_drain() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    query("INSERT INTO audit_rotation_keys (id, label, revoked) VALUES (10, 'old-key', false)")
        .execute(pool)
        .await
        .expect("seed");

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    let delivered: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let on_updated = Arc::clone(&delivered);
    cool.events().on_audit_rotation_key_updated(move |event| {
        let delivered = Arc::clone(&on_updated);
        async move {
            delivered.lock().unwrap().push(event.data.label);
            Ok(())
        }
    });
    let on_created = Arc::clone(&delivered);
    cool.events().on_audit_rotation_key_created(move |event| {
        let delivered = Arc::clone(&on_created);
        async move {
            delivered.lock().unwrap().push(event.data.label);
            Ok(())
        }
    });

    let mut tx = pool.begin().await.expect("begin caller-managed tx");
    cool.audit_rotation_key()
        .update(10)
        .set(cratestack_schema::UpdateAuditRotationKeyInput {
            label: None,
            revoked: Some(true),
        })
        .run_in_tx(&mut tx, &ctx)
        .await
        .expect("first emitting write in tx (revoke)");
    cool.audit_rotation_key()
        .create(cratestack_schema::CreateAuditRotationKeyInput {
            id: 11,
            label: "rotated-key".to_owned(),
            revoked: false,
        })
        .run_in_tx(&mut tx, &ctx)
        .await
        .expect("second emitting write in tx (create)");
    tx.commit().await.expect("commit");

    assert!(
        delivered.lock().unwrap().is_empty(),
        "run_in_tx writes must not auto-deliver to subscribers — delivery is caller-driven \
         via events().drain(), same as AuditSink dispatch is caller-driven",
    );

    let drained = cool.events().drain().await.expect("drain outbox");
    assert_eq!(
        drained, 2,
        "drain should report exactly the two events this transaction just enqueued",
    );

    let mut got = delivered.lock().unwrap().clone();
    got.sort();
    assert_eq!(
        got,
        vec!["old-key".to_owned(), "rotated-key".to_owned()],
        "both run_in_tx-composed writes' events should reach their subscribed handlers once \
         the caller drains the outbox after their own commit",
    );
}

/// **The decisive test for the (c) half of cratestack#534's follow-up.**
/// `db.transaction(...)` (cratestack#513) is the newer, "sanctioned" way
/// to compose several write-builder calls — landed after the fix above,
/// so nothing exercised this combination before. It would be easy for a
/// caller (or a future maintainer) to assume the sanctioned composition
/// API also gets automatic `AuditSink` fan-out. It does not, and this
/// pins that down: chaining the exact same two `run_in_tx` writes through
/// `cool.transaction(...)` instead of a raw `pool.begin()` transaction
/// still leaves the installed sink observing ZERO events once the
/// combinator's own commit has already happened — dispatch is still
/// entirely the caller's job. See `cratestack_sqlx::transaction`'s module
/// doc comment ("Composing through here does not close the
/// `AuditSink`/outbox gap") for the documented claim this test locks in
/// place; if this assertion ever starts failing because `transaction()`
/// began auto-dispatching, that doc comment must be corrected in the same
/// change, not left to silently go stale the way the pre-#534 docs did.
#[tokio::test]
async fn chained_db_transaction_writes_do_not_auto_dispatch_to_sink() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    query("INSERT INTO audit_rotation_keys (id, label, revoked) VALUES (20, 'old-key', false)")
        .execute(pool)
        .await
        .expect("seed");

    let sink = RecordingAuditSink::default();
    let cool = cratestack_schema::Cratestack::builder(pool.clone())
        .with_audit_sink(Arc::new(sink.clone()))
        .build();
    let ctx = operator();

    // The closure's own return type is arbitrary caller code, same as any
    // `db.transaction(...)` body — `Cratestack::transaction` has no way
    // to see inside it. This body threads the `RunInTxOutcome::audit_events`
    // out through its own `Ok(..)` precisely because nothing else would
    // ever hand them back.
    let audit_events = cool
        .transaction(async |tx| {
            let mut audit_events = Vec::new();

            let revoke = cool
                .audit_rotation_key()
                .update(20)
                .set(cratestack_schema::UpdateAuditRotationKeyInput {
                    label: None,
                    revoked: Some(true),
                })
                .run_in_tx(tx, &ctx)
                .await?;
            audit_events.extend(revoke.audit_events);

            let create = cool
                .audit_rotation_key()
                .create(cratestack_schema::CreateAuditRotationKeyInput {
                    id: 21,
                    label: "new-key".to_owned(),
                    revoked: false,
                })
                .run_in_tx(tx, &ctx)
                .await?;
            audit_events.extend(create.audit_events);

            Ok(audit_events)
        })
        .await
        .expect("both writes should commit via the transaction combinator");

    assert_eq!(
        audit_events.len(),
        2,
        "run_in_tx should still hand back one AuditEvent per @@audit write even when composed \
         through db.transaction(...)",
    );

    // *** The decisive assertion: `transaction()` has already returned
    // `Ok`, meaning its own internal `tx.commit()` already succeeded —
    // and the sink has still observed nothing. ***
    assert!(
        sink.events.lock().unwrap().is_empty(),
        "cratestack#534 (c): db.transaction(...) must NOT auto-dispatch to the installed \
         AuditSink after its own commit — this is the documented, deliberately caller-driven \
         contract, and this assertion would catch a silent regression of it becoming true \
         by accident",
    );

    // Confirming the assertion above isn't vacuous (e.g. because nothing
    // ever reaches the sink at all): the caller's own explicit dispatch
    // still works, the same as it does for a raw `pool.begin()` transaction.
    cool.dispatch_audit_sink(&audit_events).await;
    let recorded = sink.events.lock().unwrap().clone();
    assert_eq!(
        recorded.len(),
        2,
        "explicit dispatch after db.transaction(...) returns Ok must still deliver both events \
         — proves the zero-events assertion above is discriminating, not just broken plumbing",
    );
}
