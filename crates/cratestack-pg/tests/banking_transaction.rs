//! End-to-end test for `db.transaction(...)` (cratestack#513): compose two
//! writes across two different models inside one Postgres transaction using
//! only the generated `Cratestack` handle — no `cratestack::sqlx` import in
//! this file, no `sqlx::Transaction` named anywhere in it.
//!
//! The "no `sqlx` dependency in a real consumer's `Cargo.toml`" half of the
//! acceptance bar is proven separately by the standalone
//! `examples/db-transaction-verification` crate (its own `Cargo.toml` never
//! lists `sqlx`, only `cratestack`) — this file proves the transactional
//! *behavior* against a real Postgres, reusing the already-wired
//! testcontainers/`CRATESTACK_TEST_DATABASE_URL` harness the other
//! `banking_*.rs` tests share.

use cratestack::include_server_schema;
use cratestack::{CoolContext, Value};

include_server_schema!(
    "tests/fixtures/banking_transaction_combinator.cstack",
    db = Postgres
);

mod support;

use support::pg;

async fn reset_schema(pool: &cratestack::sqlx::PgPool) {
    cratestack::sqlx::query("DROP TABLE IF EXISTS transaction_notes, transaction_widgets CASCADE")
        .execute(pool)
        .await
        .expect("drop tables");
    cratestack::sqlx::query(
        "CREATE TABLE transaction_widgets (id BIGINT PRIMARY KEY, label TEXT NOT NULL)",
    )
    .execute(pool)
    .await
    .expect("create transaction_widgets");
    cratestack::sqlx::query(
        "CREATE TABLE transaction_notes (\
            id BIGINT PRIMARY KEY, \
            widget_id BIGINT NOT NULL, \
            note TEXT NOT NULL\
         )",
    )
    .execute(pool)
    .await
    .expect("create transaction_notes");
}

fn operator() -> CoolContext {
    CoolContext::authenticated([("id".to_owned(), Value::Int(1))])
}

#[tokio::test]
async fn both_writes_commit_when_the_closure_returns_ok() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    cool.transaction(async |tx| {
        cool.transaction_widget()
            .create(cratestack_schema::CreateTransactionWidgetInput {
                id: 1,
                label: "widget-1".to_owned(),
            })
            .run_in_tx(tx, &ctx)
            .await?;

        cool.transaction_note()
            .create(cratestack_schema::CreateTransactionNoteInput {
                id: 1,
                widgetId: 1,
                note: "first note".to_owned(),
            })
            .run_in_tx(tx, &ctx)
            .await?;

        Ok(())
    })
    .await
    .expect("both writes should commit");

    let widget_count: i64 =
        cratestack::sqlx::query_scalar("SELECT COUNT(*)::BIGINT FROM transaction_widgets")
            .fetch_one(pool)
            .await
            .expect("count widgets");
    let note_count: i64 =
        cratestack::sqlx::query_scalar("SELECT COUNT(*)::BIGINT FROM transaction_notes")
            .fetch_one(pool)
            .await
            .expect("count notes");

    assert_eq!(widget_count, 1, "the widget write should be committed");
    assert_eq!(note_count, 1, "the note write should be committed");
}

#[tokio::test]
async fn neither_write_is_visible_when_the_second_fails() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    // Seed a pre-existing note row at id=1 so the transaction's second
    // write collides on the primary key and fails deterministically —
    // no reliance on timing or a flaky trigger.
    cratestack::sqlx::query(
        "INSERT INTO transaction_notes (id, widget_id, note) VALUES (1, 999, 'pre-existing')",
    )
    .execute(pool)
    .await
    .expect("seed colliding note");

    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    let outcome = cool
        .transaction(async |tx| {
            // This first write succeeds at the SQL level...
            cool.transaction_widget()
                .create(cratestack_schema::CreateTransactionWidgetInput {
                    id: 2,
                    label: "widget-2".to_owned(),
                })
                .run_in_tx(tx, &ctx)
                .await?;

            // ...but this one collides on the seeded id=1 primary key and
            // fails, which must roll the whole transaction back.
            cool.transaction_note()
                .create(cratestack_schema::CreateTransactionNoteInput {
                    id: 1,
                    widgetId: 2,
                    note: "should never land".to_owned(),
                })
                .run_in_tx(tx, &ctx)
                .await?;

            Ok(())
        })
        .await;

    assert!(
        outcome.is_err(),
        "the colliding second write must surface an error",
    );

    let widget_row_exists: bool = cratestack::sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM transaction_widgets WHERE id = 2)",
    )
    .fetch_one(pool)
    .await
    .expect("check widget visibility");

    assert!(
        !widget_row_exists,
        "the first write (widget id=2) must NOT be visible after the second \
         write's failure rolled the transaction back — this is the \
         discriminating assertion: flip the seeded note id above so it no \
         longer collides and this test must fail",
    );

    let note_count_for_widget_2: i64 = cratestack::sqlx::query_scalar(
        "SELECT COUNT(*)::BIGINT FROM transaction_notes WHERE widget_id = 2",
    )
    .fetch_one(pool)
    .await
    .expect("count notes for widget 2");
    assert_eq!(
        note_count_for_widget_2, 0,
        "no note referencing widget id=2 should exist after rollback",
    );

    // The pre-existing seeded row (untouched by the failed transaction)
    // must still be exactly what we seeded — confirms the rollback didn't
    // also revert unrelated prior state.
    let seeded_widget_id: i64 =
        cratestack::sqlx::query_scalar("SELECT widget_id FROM transaction_notes WHERE id = 1")
            .fetch_one(pool)
            .await
            .expect("read seeded note");
    assert_eq!(seeded_widget_id, 999);
}
