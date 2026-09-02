//! cratestack#867 — the declarative `query` block, end to end against a
//! real Postgres (accepted design `docs/design/declarative-custom-query.md`).
//!
//! What each test is actually pinning down, so none of them can be
//! weakened into a tautology later:
//!
//! 1. `runs_the_motivating_two_aggregate_filter_query` — the query epic
//!    #488 was opened against. Two aggregates in one row, one of them
//!    `FILTER (WHERE …)`-qualified, with a caller-supplied cutoff. The
//!    generated aggregate builder can express none of that
//!    (`cratestack-sqlx/src/query/read/aggregate.rs` is one column and one
//!    aggregate per round trip), which is the whole reason the construct
//!    exists. The assertion is on *values*, not on "it didn't error" —
//!    `thisMonth` must exclude the pre-cutoff row, so a `FILTER` clause
//!    that silently stopped applying would fail here.
//! 2. `denies_a_principal_the_policy_does_not_admit` — `@allow(auth() !=
//!    null && auth().subjectId == userId)` must reject a caller asking
//!    about someone else's rows, with `Forbidden`, before any SQL runs.
//! 3. `denies_everyone_when_no_allow_is_declared` — deny-by-default. The
//!    schema's `unreachableSummary` has no `@allow` at all; an
//!    authenticated caller must still be refused.
//! 4. `runs_a_list_returning_query` — the `fetch_all` path, over a
//!    `GROUP BY … HAVING` the builder also cannot express.
//! 5. `runs_a_parameterless_query` — design §8's self-test: no `$N`, no
//!    special case.
//! 6. `a_data_modifying_cte_is_refused_by_the_database` — the review
//!    finding that a `query` body could WRITE. A data-modifying CTE is an
//!    ordinary `SELECT` to the driver, so the guarantee cannot come from
//!    the statement; it comes from the `READ ONLY` transaction the
//!    generated `run` opens. The assertion is on the row count as well as
//!    the error, because "it returned Err" would also be true of a
//!    statement that wrote and *then* failed.
//! 7. `a_system_principal_satisfies_auth_is_system` — `@allow(auth()
//!    .isSystem())` had no arm on this policy dialect and failed to
//!    compile at all. The reconciliation query that motivated the whole
//!    feature needs it.
//! 8. `a_query_does_not_observe_an_enclosing_transaction` — pins the
//!    documented limitation rather than leaving it to be discovered: a
//!    query runs on its own pooled connection.
//!
//! Skips (prints `ok` in ~0.00s) without a database — set
//! `CRATESTACK_REQUIRE_DB=1` to make that a hard failure instead. Read
//! `finished in` rather than the summary line to tell a skip from a pass.

use cratestack::sqlx::{PgPool, query};
use cratestack::{CratestackContext, CratestackError, SystemContext, Value};
use cratestack::{include_client_schema, include_server_schema};

include_server_schema!("tests/fixtures/declarative_query.cstack", db = Postgres);

mod support;

use support::pg;

async fn reset_schema(pool: &PgPool) {
    query("DROP TABLE IF EXISTS loyalty_fee_events")
        .execute(pool)
        .await
        .expect("drop table");
    query(
        "CREATE TABLE loyalty_fee_events (
            id BIGINT PRIMARY KEY,
            user_id TEXT NOT NULL,
            discount BIGINT NOT NULL,
            created_at TIMESTAMPTZ NOT NULL
        )",
    )
    .execute(pool)
    .await
    .expect("create table");
    query(
        "INSERT INTO loyalty_fee_events (id, user_id, discount, created_at) VALUES
            (1, 'user-7', 100, TIMESTAMPTZ '2026-01-10 00:00:00Z'),
            (2, 'user-7', 250, TIMESTAMPTZ '2026-03-05 00:00:00Z'),
            (3, 'user-7',  30, TIMESTAMPTZ '2026-03-20 00:00:00Z'),
            (4, 'user-9', 999, TIMESTAMPTZ '2026-03-21 00:00:00Z')",
    )
    .execute(pool)
    .await
    .expect("seed");
}

fn operator(subject: &str) -> CratestackContext {
    CratestackContext::authenticated([("subjectId".to_owned(), Value::String(subject.to_owned()))])
}

/// 2026-03-01, the cutoff the `FILTER` clause compares against.
fn cutoff() -> cratestack::chrono::DateTime<cratestack::chrono::Utc> {
    "2026-03-01T00:00:00Z".parse().expect("cutoff should parse")
}

#[tokio::test]
async fn runs_the_motivating_two_aggregate_filter_query() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    reset_schema(&test_pg.pool).await;

    let db = cratestack_schema::Cratestack::builder(test_pg.pool.clone()).build();
    let summary = db
        .queries()
        .loyalty_fee_summary(
            &cratestack_schema::queries::loyalty_fee_summary::Args {
                userId: "user-7".to_owned(),
                cutoff: cutoff(),
            },
            &operator("user-7"),
        )
        .await
        .expect("an admitted principal should get rows");

    // 100 + 250 + 30 across all time; only 250 + 30 land on/after the
    // cutoff. `user-9`'s 999 belongs to nobody in this call.
    assert_eq!(summary.total, 380);
    assert_eq!(summary.thisMonth, 280);
}

#[tokio::test]
async fn denies_a_principal_the_policy_does_not_admit() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    reset_schema(&test_pg.pool).await;

    let db = cratestack_schema::Cratestack::builder(test_pg.pool.clone()).build();
    let outcome = db
        .queries()
        .loyalty_fee_summary(
            &cratestack_schema::queries::loyalty_fee_summary::Args {
                userId: "user-7".to_owned(),
                cutoff: cutoff(),
            },
            // Authenticated, but as somebody else — the `@allow` compares
            // `auth().subjectId` against the query's own `userId` argument.
            &operator("user-9"),
        )
        .await;

    assert!(
        matches!(outcome, Err(CratestackError::Forbidden(_))),
        "expected Forbidden, got {outcome:?}",
    );
}

#[tokio::test]
async fn denies_an_unauthenticated_caller() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    reset_schema(&test_pg.pool).await;

    let db = cratestack_schema::Cratestack::builder(test_pg.pool.clone()).build();
    let outcome = db
        .queries()
        .loyalty_fee_summary(
            &cratestack_schema::queries::loyalty_fee_summary::Args {
                userId: "user-7".to_owned(),
                cutoff: cutoff(),
            },
            &CratestackContext::anonymous(),
        )
        .await;

    assert!(
        matches!(outcome, Err(CratestackError::Forbidden(_))),
        "expected Forbidden, got {outcome:?}",
    );
}

#[tokio::test]
async fn denies_everyone_when_no_allow_is_declared() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    reset_schema(&test_pg.pool).await;

    let db = cratestack_schema::Cratestack::builder(test_pg.pool.clone()).build();
    let outcome = db
        .queries()
        .unreachable_summary(
            &cratestack_schema::queries::unreachable_summary::Args {
                userId: "user-7".to_owned(),
            },
            // A fully authenticated principal, and still refused: an empty
            // `ALLOW_POLICIES` denies, it does not permit.
            &operator("user-7"),
        )
        .await;

    assert!(
        matches!(outcome, Err(CratestackError::Forbidden(_))),
        "expected Forbidden from a query with no @allow, got {outcome:?}",
    );
}

#[tokio::test]
async fn runs_a_list_returning_query() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    reset_schema(&test_pg.pool).await;

    let db = cratestack_schema::Cratestack::builder(test_pg.pool.clone()).build();
    let rows = db
        .queries()
        .loyalty_leaderboard(
            &cratestack_schema::queries::loyalty_leaderboard::Args { minTotal: 100 },
            &operator("user-7"),
        )
        .await
        .expect("list query should run");

    assert_eq!(
        rows.iter()
            .map(|row| (row.userId.as_str(), row.total))
            .collect::<Vec<_>>(),
        vec![("user-9", 999), ("user-7", 380)],
    );
}

#[tokio::test]
async fn runs_a_parameterless_query() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    reset_schema(&test_pg.pool).await;

    let db = cratestack_schema::Cratestack::builder(test_pg.pool.clone()).build();
    let summary = db
        .queries()
        .loyalty_event_count(
            &cratestack_schema::queries::loyalty_event_count::Args::default(),
            &operator("user-7"),
        )
        .await
        .expect("parameterless query should run");

    assert_eq!(summary.total, 4);
}

/// The SQL body reaches the generated code byte-for-byte, which is what
/// makes "parameters are bound, never interpolated" checkable rather than
/// merely asserted: there is no rewriting step between the schema text and
/// this constant.
#[test]
fn the_generated_sql_is_the_schema_text_verbatim() {
    let sql = cratestack_schema::queries::loyalty_fee_summary::SQL;
    assert!(sql.contains("FILTER (WHERE created_at >= $2)"), "{sql}");
    assert!(sql.contains("WHERE user_id = $1"), "{sql}");
    assert!(
        !sql.contains("user-7"),
        "no argument value may ever appear in the statement text",
    );
}

/// The Rust client's half of "a `query` generates no client surface"
/// (design §5). The Dart and TypeScript halves are proved by byte-equality
/// against a query-free twin schema in
/// `cratestack-client-{dart,typescript}/tests/declarative_query_absent.rs`;
/// the Rust client generator emits into a macro expansion, so the
/// equivalent assertion is made on the surface it publishes.
///
/// Two things are being pinned, and the second is the one that matters:
///
/// 1. `include_client_schema!` **accepts** a query-bearing schema. A query
///    is invisible to a client, not an error for one — a shared schema
///    file has to stay usable from a client crate after someone adds a
///    query to it. This module merely compiling is that assertion.
/// 2. None of the four queries became client surface. `PROCEDURES` is
///    where a procedure-shaped construct would land, and it is empty; the
///    result `type`s are still present because a declared `type` is
///    ordinary client surface whether or not a query returns one.
mod generated_client {
    use super::include_client_schema;

    include_client_schema!("tests/fixtures/declarative_query.cstack");
}

#[test]
fn the_rust_client_generates_no_surface_for_a_query() {
    assert!(
        generated_client::cratestack_schema::PROCEDURES.is_empty(),
        "a query must not become a client procedure stub: {:?}",
        generated_client::cratestack_schema::PROCEDURES,
    );
    assert_eq!(
        generated_client::cratestack_schema::MODELS,
        ["LoyaltyFeeEvent"],
    );
    assert!(
        generated_client::cratestack_schema::TYPES.contains(&"LoyaltyFeeSummary"),
        "a declared `type` stays client surface even when a query returns it",
    );
}

#[tokio::test]
async fn a_data_modifying_cte_is_refused_by_the_database() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    reset_schema(&test_pg.pool).await;

    let db = cratestack_schema::Cratestack::builder(test_pg.pool.clone()).build();
    let outcome = db
        .queries()
        .sneaky_write(
            &cratestack_schema::queries::sneaky_write::Args {
                userId: "user-7".to_owned(),
            },
            // Deliberately a principal the `@allow` ADMITS. The point is
            // that policy is not what stops this — the read-only
            // transaction is.
            &operator("user-7"),
        )
        .await;

    assert!(
        matches!(outcome, Err(CratestackError::Internal(_))),
        "a data-modifying CTE must be refused, got {outcome:?}",
    );
    let message = outcome.unwrap_err().to_string();
    assert!(
        message.contains("read-only transaction"),
        "the error should say why, got: {message}",
    );
    assert!(
        !message.contains("INSERT INTO"),
        "the public message must not echo the schema's SQL, got: {message}",
    );

    // The decisive half: nothing was written. An error alone would also
    // be produced by a statement that inserted and then failed.
    let count: i64 =
        cratestack::sqlx::query_scalar("SELECT COUNT(*)::BIGINT FROM loyalty_fee_events")
            .fetch_one(&test_pg.pool)
            .await
            .expect("count rows");
    assert_eq!(count, 4, "the seeded row count must be unchanged");
}

#[tokio::test]
async fn an_ordinary_select_still_works_under_the_read_only_transaction() {
    // Guards the test above against passing for the wrong reason: if the
    // read-only transaction broke *every* query, `sneakyWrite` would
    // still fail and prove nothing.
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    reset_schema(&test_pg.pool).await;

    let db = cratestack_schema::Cratestack::builder(test_pg.pool.clone()).build();
    let summary = db
        .queries()
        .loyalty_event_count(
            &cratestack_schema::queries::loyalty_event_count::Args::default(),
            &operator("user-7"),
        )
        .await
        .expect("a plain SELECT must still run inside the read-only transaction");

    assert_eq!(summary.total, 4);
}

#[tokio::test]
async fn a_system_principal_satisfies_auth_is_system() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    reset_schema(&test_pg.pool).await;

    let db = cratestack_schema::Cratestack::builder(test_pg.pool.clone()).build();
    let args = cratestack_schema::queries::system_only_totals::Args {
        userId: "user-7".to_owned(),
    };

    let allowed = db
        .queries()
        .system_only_totals(
            &args,
            &SystemContext::for_service("ledger-worker").into_context(),
        )
        .await
        .expect("a system principal should satisfy auth().isSystem()");
    assert_eq!(allowed.total, 380);

    // Fail-closed: an ordinary authenticated caller is not a system one,
    // and `isSystem()` must not be satisfiable by asserting a claim.
    let denied = db
        .queries()
        .system_only_totals(&args, &operator("user-7"))
        .await;
    assert!(
        matches!(denied, Err(CratestackError::Forbidden(_))),
        "an ordinary principal must not satisfy isSystem(), got {denied:?}",
    );
}

#[tokio::test]
async fn the_denial_message_names_the_query_not_a_procedure() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    reset_schema(&test_pg.pool).await;

    let db = cratestack_schema::Cratestack::builder(test_pg.pool.clone()).build();
    let error = db
        .queries()
        .unreachable_summary(
            &cratestack_schema::queries::unreachable_summary::Args {
                userId: "user-7".to_owned(),
            },
            &operator("user-7"),
        )
        .await
        .expect_err("a policy-less query denies everyone");

    let message = error.to_string();
    assert!(
        message.contains("query policy denied"),
        "a denied query should not report a procedure, got: {message}",
    );
}

/// A `query` runs on its own pooled connection, so it does **not** observe
/// uncommitted writes made by an enclosing `Cratestack::transaction(...)`.
///
/// This pins a documented limitation rather than a desired behaviour. It
/// is here so the limitation is measured and stays measured: if a future
/// change makes a query join the ambient transaction, this test fails and
/// forces the doc comment, the design doc and the changelog to be updated
/// with it — which is exactly what did not happen when the gap was
/// introduced.
#[tokio::test]
async fn a_query_does_not_observe_an_enclosing_transaction() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    reset_schema(&test_pg.pool).await;

    let db = cratestack_schema::Cratestack::builder(test_pg.pool.clone()).build();
    let ctx = operator("user-7");

    let seen_inside = db
        .transaction(async |tx| {
            cratestack::sqlx::query(
                "INSERT INTO loyalty_fee_events (id, user_id, discount, created_at) \
                 VALUES (99, 'user-7', 5000, NOW())",
            )
            .execute(&mut ***tx)
            .await
            .expect("write inside the transaction");

            let inside = db
                .queries()
                .loyalty_event_count(
                    &cratestack_schema::queries::loyalty_event_count::Args::default(),
                    &ctx,
                )
                .await
                .expect("the query itself should succeed");
            Ok(inside.total)
        })
        .await
        .expect("transaction should commit");

    // 4, not 5: the uncommitted insert is invisible to the query.
    assert_eq!(
        seen_inside, 4,
        "a query must not observe an enclosing transaction's uncommitted writes",
    );

    // And after commit it is visible, which is what makes the assertion
    // above a statement about isolation rather than about a broken query.
    let after = db
        .queries()
        .loyalty_event_count(
            &cratestack_schema::queries::loyalty_event_count::Args::default(),
            &ctx,
        )
        .await
        .expect("query after commit");
    assert_eq!(after.total, 5);
}
