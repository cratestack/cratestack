//! Real-Postgres proof for cratestack#513: `db.transaction(...)` commits
//! both writes together and rolls back both when the second one fails.
//!
//! **What this file does and does not prove about `sqlx`:** the crate's
//! `Cargo.toml` never lists `sqlx` as a dependency — every `cratestack::
//! sqlx::` path below resolves through this crate's one real dependency,
//! `cratestack` (`cratestack-pg`), the same re-export
//! `crates/cratestack-pg/tests/banking_*.rs` uses for raw fixture setup.
//! This file *does* use that path for table DDL and read-back assertions,
//! same as those framework-internal tests do — that's expected and fine.
//! The actual acceptance-bar claim ("no `sqlx::Transaction` named, no
//! `sqlx` type in the transaction call site") is about `src/lib.rs`'s
//! [`db_transaction_verification::create_widget_with_note`], which this
//! file only calls, never reimplements.
//!
//! Skips (prints `ok` without exercising anything) if Docker isn't
//! available — set `CRATESTACK_REQUIRE_DB=1` to turn that into a hard
//! panic instead, same convention as `crates/cratestack-pg/tests/support/
//! pg.rs`.

use db_transaction_verification::{create_widget_with_note, schema};
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;

async fn connect_or_skip() -> Option<cratestack::sqlx::PgPool> {
    let require = std::env::var("CRATESTACK_REQUIRE_DB").is_ok();

    fn need<T, E: std::fmt::Display>(r: Result<T, E>, require: bool, ctx: &str) -> Option<T> {
        match r {
            Ok(v) => Some(v),
            Err(e) if require => panic!("CRATESTACK_REQUIRE_DB is set but {ctx} failed: {e}"),
            Err(_) => None,
        }
    }

    let container = need(
        Postgres::default().start().await,
        require,
        "starting the Postgres testcontainer (is Docker available?)",
    )?;
    let host = need(container.get_host().await, require, "resolving host")?;
    let port = need(
        container.get_host_port_ipv4(5432).await,
        require,
        "resolving port",
    )?;
    let url = format!("postgres://postgres:postgres@{host}:{port}/postgres");
    let pool = need(
        cratestack::sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await,
        require,
        "connecting to the testcontainer",
    )?;
    // Leak the container so it stays alive for the test body — this test
    // binary runs one test per process invocation via `cargo test`'s
    // default harness, so the leak is bounded and cleaned up when the
    // process exits.
    std::mem::forget(container);
    Some(pool)
}

async fn create_tables(pool: &cratestack::sqlx::PgPool) {
    cratestack::sqlx::query("CREATE TABLE widgets (id BIGINT PRIMARY KEY, label TEXT NOT NULL)")
        .execute(pool)
        .await
        .expect("create widgets");
    cratestack::sqlx::query(
        "CREATE TABLE widget_notes (\
            id BIGINT PRIMARY KEY, \
            widget_id BIGINT NOT NULL, \
            note TEXT NOT NULL\
         )",
    )
    .execute(pool)
    .await
    .expect("create widget_notes");
}

fn operator() -> cratestack::CratestackContext {
    cratestack::CratestackContext::authenticated([("id".to_owned(), cratestack::Value::Int(1))])
}

#[tokio::test]
async fn both_writes_commit_together() {
    let Some(pool) = connect_or_skip().await else {
        return;
    };
    create_tables(&pool).await;

    let db = schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    create_widget_with_note(&db, &ctx, 1, "widget-1".to_owned(), 1, "note-1".to_owned())
        .await
        .expect("both writes should commit");

    let widget_count: i64 = cratestack::sqlx::query_scalar("SELECT COUNT(*)::BIGINT FROM widgets")
        .fetch_one(&pool)
        .await
        .expect("count widgets");
    let note_count: i64 =
        cratestack::sqlx::query_scalar("SELECT COUNT(*)::BIGINT FROM widget_notes")
            .fetch_one(&pool)
            .await
            .expect("count notes");

    assert_eq!(widget_count, 1);
    assert_eq!(note_count, 1);
}

#[tokio::test]
async fn neither_write_is_visible_when_the_second_fails() {
    let Some(pool) = connect_or_skip().await else {
        return;
    };
    create_tables(&pool).await;

    // Seed a colliding note id so the transaction's second write fails
    // deterministically.
    cratestack::sqlx::query(
        "INSERT INTO widget_notes (id, widget_id, note) VALUES (1, 999, 'pre-existing')",
    )
    .execute(&pool)
    .await
    .expect("seed collision");

    let db = schema::Cratestack::builder(pool.clone()).build();
    let ctx = operator();

    let outcome = create_widget_with_note(
        &db,
        &ctx,
        2,
        "widget-2".to_owned(),
        1, // collides with the seeded row above
        "should never land".to_owned(),
    )
    .await;

    assert!(
        outcome.is_err(),
        "the colliding write must surface an error"
    );

    let widget_row_exists: bool =
        cratestack::sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM widgets WHERE id = 2)")
            .fetch_one(&pool)
            .await
            .expect("check widget visibility");

    assert!(
        !widget_row_exists,
        "the first write must not be visible after the transaction rolled back",
    );
}
