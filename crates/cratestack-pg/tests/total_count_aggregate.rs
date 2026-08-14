//! Regression test for cratestack#570: a `@@paged` model's list route
//! used to compute `total_count` by re-running the filtered query with
//! paging disabled and calling `.len()` on the decoded rows — a full
//! materialising `SELECT` issued purely to read a length. Fixed by
//! converting the same `FindMany` the page query builds into an
//! `AggregateCount` (`From<FindMany>` in
//! `cratestack-sqlx/src/query/read/aggregate_count.rs`), so the count
//! is a real `COUNT(*)` that reuses the identical `WHERE`/policy scope.
//!
//! The whole risk here is a *divergent* count — one that includes rows
//! the caller's read policy wouldn't let them see. Every test below
//! seeds admitted rows != total rows (via row-scoped `@@allow` and/or
//! `@@soft_delete`) specifically so a count that silently fell back to
//! "the whole table" would be caught.

use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::include_server_schema;
use cratestack::sqlx::postgres::PgPoolOptions;
use cratestack::{AuthProvider, CratestackCodec, CratestackContext, RequestContext, Value};
use cratestack_codec_json::JsonCodec;
use tower::util::ServiceExt;

include_server_schema!("tests/fixtures/total_count_aggregate.cstack", db = Postgres);

mod support;

use support::{pg, tracing_capture};

#[derive(Clone)]
struct OwnerAuthProvider;

impl AuthProvider for OwnerAuthProvider {
    type Error = cratestack::CratestackError;

    fn authenticate(
        &self,
        request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        let id = request
            .headers
            .get("x-auth-id")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<i64>().ok());

        core::future::ready(Ok(match id {
            Some(id) => CratestackContext::authenticated([("id".to_owned(), Value::Int(id))]),
            None => CratestackContext::anonymous(),
        }))
    }
}

fn router(pool: cratestack::sqlx::PgPool) -> cratestack::axum::Router {
    cratestack_schema::axum::model_router(
        cratestack_schema::Cratestack::builder(pool).build(),
        JsonCodec,
        OwnerAuthProvider,
    )
}

async fn reset_schema(pool: &cratestack::sqlx::PgPool) {
    cratestack::sqlx::query("DROP TABLE IF EXISTS total_count_items")
        .execute(pool)
        .await
        .expect("drop total_count_items table");
    cratestack::sqlx::query(
        "CREATE TABLE total_count_items (
            id BIGINT PRIMARY KEY,
            label TEXT NOT NULL,
            owner_id BIGINT NOT NULL,
            deleted_at TIMESTAMPTZ
        )",
    )
    .execute(pool)
    .await
    .expect("create total_count_items table");
}

async fn seed_item(pool: &cratestack::sqlx::PgPool, id: i64, owner_id: i64, deleted: bool) {
    let deleted_expr = if deleted { "NOW()" } else { "NULL" };
    cratestack::sqlx::query(&format!(
        "INSERT INTO total_count_items (id, label, owner_id, deleted_at) \
         VALUES ({id}, 'item-{id}', {owner_id}, {deleted_expr})"
    ))
    .execute(pool)
    .await
    .expect("seed item");
}

#[tokio::test]
async fn total_count_reflects_policy_scoped_rows_not_the_whole_table() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    // 7 rows admitted (owner 1), 5 rows not admitted (owner 2). A count
    // that fell back to `SELECT COUNT(*) FROM total_count_items` with no policy
    // scoping would report 12, not 7 — the exact divergence #570's
    // acceptance criteria call a data-leak-shaped bug.
    for id in 1..=7 {
        seed_item(pool, id, 1, false).await;
    }
    for id in 8..=12 {
        seed_item(pool, id, 2, false).await;
    }

    let response = router(pool.clone())
        .oneshot(
            Request::get("/total_count_items?limit=3")
                .header("x-auth-id", "1")
                .header("accept", JsonCodec::CONTENT_TYPE)
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should complete");

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body should read");
    let page: cratestack::Page<cratestack_schema::TotalCountItem> =
        JsonCodec.decode(&body).expect("page should decode");

    assert_eq!(
        page.items.len(),
        3,
        "limit must still cap the returned page"
    );
    assert_eq!(
        page.total_count,
        Some(7),
        "total_count must equal the policy-admitted count (7), not the whole table (12)",
    );
    assert!(
        page.page_info.has_next_page,
        "3 returned of 7 admitted means more pages remain",
    );
    assert!(!page.page_info.has_previous_page);
}

#[tokio::test]
async fn total_count_excludes_soft_deleted_rows_for_the_admitted_owner() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;

    // Owner 1 (admitted): 3 alive + 2 tombstoned -> 3 should count.
    for id in 1..=3 {
        seed_item(pool, id, 1, false).await;
    }
    for id in 4..=5 {
        seed_item(pool, id, 1, true).await;
    }
    // Owner 2 (not admitted at all), alive.
    for id in 6..=9 {
        seed_item(pool, id, 2, false).await;
    }

    let response = router(pool.clone())
        .oneshot(
            Request::get("/total_count_items?limit=10")
                .header("x-auth-id", "1")
                .header("accept", JsonCodec::CONTENT_TYPE)
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should complete");

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body should read");
    let page: cratestack::Page<cratestack_schema::TotalCountItem> =
        JsonCodec.decode(&body).expect("page should decode");

    assert_eq!(page.items.len(), 3);
    assert_eq!(
        page.total_count,
        Some(3),
        "total_count must exclude both non-admitted rows and soft-deleted admitted rows",
    );
}

#[tokio::test]
async fn total_count_query_is_a_count_aggregate_not_a_row_materializing_select() {
    // No live DB needed: `preview_scoped_sql` builds the same
    // `sqlx::QueryBuilder` `run`/`run_in_tx` use, without executing it.
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://cratestack:cratestack@localhost/cratestack")
        .expect("lazy pool should parse");
    let db = cratestack_schema::Cratestack::builder(pool).build();
    let ctx = CratestackContext::authenticated([("id".to_owned(), Value::Int(1))]);

    let count_request = cratestack::AggregateCount::from(db.total_count_item().find_many());
    let sql = count_request.preview_scoped_sql(&ctx);

    assert!(
        sql.starts_with("SELECT COUNT(*) FROM total_count_items"),
        "count path must be an aggregate, not a row-selecting query: {sql}",
    );
    assert!(
        sql.contains("deleted_at IS NULL"),
        "must apply the same soft-delete scoping the page query uses: {sql}",
    );
    assert!(
        sql.contains("owner_id = "),
        "must apply the same read-policy scoping the page query uses: {sql}",
    );
    assert!(
        !sql.contains("label"),
        "must not select/project any model column: {sql}",
    );
    assert!(
        !sql.to_uppercase().contains("LIMIT"),
        "a total-count query has no page bound: {sql}",
    );
}

#[tokio::test]
async fn list_route_issues_a_count_aggregate_against_real_postgres() {
    // The decisive proof: assert on the SQL the real generated route
    // actually sends to Postgres, not just on the primitive in
    // isolation. A timing/row-count observation would be weaker (and
    // wouldn't distinguish "fast because the table is tiny" from "fast
    // because it's a real aggregate").
    tracing_capture::init_tracing();
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    for id in 1..=5 {
        seed_item(pool, id, 1, false).await;
    }

    let (response, events) = tracing_capture::capture_events(
        router(pool.clone()).oneshot(
            Request::get("/total_count_items?limit=2")
                .header("x-auth-id", "1")
                .header("accept", JsonCodec::CONTENT_TYPE)
                .body(Body::empty())
                .expect("request should build"),
        ),
    )
    .await;
    let response = response.expect("request should complete");
    assert_eq!(response.status(), StatusCode::OK);

    let items_sql_events: Vec<String> = events
        .into_iter()
        .filter(|line| line.contains("FROM total_count_items"))
        .collect();

    let count_events: Vec<&String> = items_sql_events
        .iter()
        .filter(|line| line.contains("COUNT("))
        .collect();
    assert_eq!(
        count_events.len(),
        1,
        "expected exactly one COUNT(*) aggregate statement against total_count_items, got: {items_sql_events:#?}",
    );
    assert!(
        !count_events[0].contains("\\\"label\\\""),
        "the count statement must not select the model's columns: {}",
        count_events[0],
    );

    // Every remaining (non-count) statement touching `total_count_items` must be
    // the bounded page query — proves the old unbounded
    // "SELECT <cols> FROM total_count_items WHERE ..." materializing query (no
    // LIMIT) is gone, not just that a COUNT happens to also run.
    let non_count_events: Vec<&String> = items_sql_events
        .iter()
        .filter(|line| !line.contains("COUNT("))
        .collect();
    assert!(
        !non_count_events.is_empty(),
        "expected the paginated page-select statement to also be captured",
    );
    for statement in &non_count_events {
        assert!(
            statement.contains("LIMIT"),
            "every non-count SELECT against total_count_items in this request must be the bounded page \
             query, not an unbounded materializing one: {statement}",
        );
    }
}
