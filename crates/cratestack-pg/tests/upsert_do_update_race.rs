//! `.upsert(..).run(..)` (the `ON CONFLICT ... DO UPDATE` path) must
//! classify Created-vs-Updated from what the database actually did, not
//! from a pre-lock probe that a concurrent transaction can invalidate
//! (cratestack#745).
//!
//! `lost_conflict_race_audits_an_update_not_a_create` is the acceptance
//! test. It is deterministic by construction — see
//! `tests/support/race.rs` for why the ordering is enforced by real
//! Postgres blocking rather than by sleeping — and it was confirmed
//! FAILING on the parent commit (`operation` was `create`, the outbox
//! carried `created`, and the audit `before` snapshot was `null`) before
//! the fix landed; see the PR description for the captured output.
//!
//! Skips quietly when neither `CRATESTACK_TEST_DATABASE_URL` nor
//! `CRATESTACK_USE_TESTCONTAINERS` is set (see `tests/support/pg.rs`).

mod support;

use cratestack::include_server_schema;
use cratestack::sqlx::{Row, query};
use cratestack::{CratestackContext, UpsertOutcome, Value};
use support::{pg, race};

include_server_schema!("tests/fixtures/upsert_do_update_race.cstack", db = Postgres);

const WINNER_INSERT: &str =
    "INSERT INTO race_rows (id, payload, version) VALUES ('key-1', 'winner', 0)";

async fn reset_schema(pool: &cratestack::sqlx::PgPool) {
    query("DROP TABLE IF EXISTS cratestack_audit, cratestack_event_outbox, race_rows")
        .execute(pool)
        .await
        .expect("drop tables");
    query(
        "CREATE TABLE race_rows (
            id TEXT PRIMARY KEY,
            payload TEXT NOT NULL,
            version BIGINT NOT NULL
        )",
    )
    .execute(pool)
    .await
    .expect("create race_rows");
}

fn operator() -> CratestackContext {
    CratestackContext::authenticated([("id".to_owned(), Value::Int(1))])
        .with_request_id("issue-745")
}

fn input(id: &str, payload: &str) -> cratestack_schema::CreateRaceRowInput {
    cratestack_schema::CreateRaceRowInput {
        id: id.to_owned(),
        payload: payload.to_owned(),
    }
}

async fn audit_rows(
    pool: &cratestack::sqlx::PgPool,
) -> Vec<(String, Option<String>, Option<String>)> {
    query(
        "SELECT operation, before ->> 'payload' AS before_payload, \
                after ->> 'payload' AS after_payload \
         FROM cratestack_audit WHERE model = 'RaceRow' ORDER BY occurred_at",
    )
    .fetch_all(pool)
    .await
    .expect("read cratestack_audit")
    .into_iter()
    .map(|row| {
        (
            row.get::<String, _>("operation"),
            row.get::<Option<String>, _>("before_payload"),
            row.get::<Option<String>, _>("after_payload"),
        )
    })
    .collect()
}

async fn outbox_operations(pool: &cratestack::sqlx::PgPool) -> Vec<String> {
    query(
        "SELECT operation FROM cratestack_event_outbox \
         WHERE model = 'RaceRow' ORDER BY occurred_at",
    )
    .fetch_all(pool)
    .await
    .expect("read cratestack_event_outbox")
    .into_iter()
    .map(|row| row.get::<String, _>("operation"))
    .collect()
}

async fn row_count(pool: &cratestack::sqlx::PgPool) -> i64 {
    query("SELECT COUNT(*) FROM race_rows")
        .fetch_one(pool)
        .await
        .expect("count race_rows")
        .get(0)
}

// ───── #1 the acceptance-bar regression test ─────────────────────────────

#[tokio::test]
async fn lost_conflict_race_audits_an_update_not_a_create() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    let winner = race::hold_uncommitted_conflict(&test_pg.url, "race_rows", WINNER_INSERT).await;

    let loser = cool.race_row().upsert(input("key-1", "loser")).run(&ctx);
    let (record, ()) = tokio::join!(loser, winner.commit_once_loser_blocks());
    let record = record.expect("the losing upsert still succeeds");

    // The database's own attestation that the statement took the UPDATE
    // branch: `@version` is bumped by `DO UPDATE`'s `version = version +
    // 1` clause and is never bumped by an INSERT, which writes the
    // supplied 0. If this is 0 the race did not happen and every
    // assertion below would be vacuous.
    assert_eq!(
        record.version, 1,
        "the loser's statement must have UPDATEd the winner's row, not inserted"
    );
    assert_eq!(
        record.payload, "loser",
        "DO UPDATE merges the loser's values"
    );
    assert_eq!(
        row_count(pool).await,
        1,
        "exactly one row at the conflict key"
    );

    // THE REGRESSION ASSERTIONS. Before cratestack#745 these read
    // `("create", None, Some("loser"))` / `["created"]`: the pre-lock
    // probe had already fixed `inserted = true` and nothing ever
    // reconciled it against the UPDATE the database actually performed.
    assert_eq!(
        audit_rows(pool).await,
        vec![(
            "update".to_owned(),
            Some("winner".to_owned()),
            Some("loser".to_owned())
        )],
        "a lost race must audit an update carrying the winner's row as its before-snapshot"
    );
    assert_eq!(
        outbox_operations(pool).await,
        vec!["updated".to_owned()],
        "a lost race must emit Updated, never Created"
    );
}

// ───── #2 `.do_nothing()` is unaffected by the same race ────────────────

#[tokio::test]
async fn lost_conflict_race_leaves_do_nothing_untouched() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    let winner = race::hold_uncommitted_conflict(&test_pg.url, "race_rows", WINNER_INSERT).await;

    let loser = cool
        .race_row()
        .upsert(input("key-1", "loser"))
        .do_nothing()
        .run(&ctx);
    let (outcome, ()) = tokio::join!(loser, winner.commit_once_loser_blocks());
    let outcome = outcome.expect("the losing do_nothing upsert still succeeds");

    let UpsertOutcome::Existing(existing) = outcome else {
        panic!("losing the race must report Existing, got {outcome:?}");
    };
    assert_eq!(existing.payload, "winner", "the winner's row is untouched");
    assert_eq!(
        existing.version, 0,
        "DO NOTHING must not bump @version on the existing row"
    );
    assert_eq!(row_count(pool).await, 1);

    // Nothing changed, so there is nothing to audit and nothing to emit
    // — exactly as `upsert_do_nothing.rs` asserts for the uncontended
    // conflict. cratestack#745 must not perturb this path.
    assert!(audit_rows(pool).await.is_empty());
    assert!(outbox_operations(pool).await.is_empty());
}

// ───── #3 the uncontended paths are byte-for-byte what they were ────────

#[tokio::test]
async fn uncontended_insert_and_update_are_unchanged() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    let created = cool
        .race_row()
        .upsert(input("key-2", "first"))
        .run(&ctx)
        .await
        .expect("uncontended insert branch");
    assert_eq!(created.payload, "first");
    assert_eq!(created.version, 0, "an insert writes the supplied @version");

    let updated = cool
        .race_row()
        .upsert(input("key-2", "second"))
        .run(&ctx)
        .await
        .expect("uncontended update branch");
    assert_eq!(updated.payload, "second");
    assert_eq!(updated.version, 1, "DO UPDATE bumps @version");
    assert_eq!(row_count(pool).await, 1);

    assert_eq!(
        audit_rows(pool).await,
        vec![
            ("create".to_owned(), None, Some("first".to_owned())),
            (
                "update".to_owned(),
                Some("first".to_owned()),
                Some("second".to_owned())
            ),
        ],
        "the uncontended paths must audit exactly what they audited before cratestack#745"
    );
    assert_eq!(
        outbox_operations(pool).await,
        vec!["created".to_owned(), "updated".to_owned()],
    );
}
