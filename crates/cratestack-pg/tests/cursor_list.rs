//! Keyset (cursor) pagination on the generated REST list route
//! (cratestack#1304): stable across inserts, multi-column sort with a NULLable
//! column, row policy honoured, and the refusal cases. PG-backed: skips
//! without `CRATESTACK_TEST_DATABASE_URL`.

use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::include_server_schema;
use cratestack::{AuthProvider, CratestackContext, RequestContext, Value};
use cratestack_codec_json::JsonCodec;
use serde_json::Value as Json;
use tower::util::ServiceExt;

include_server_schema!("tests/fixtures/cursor_list.cstack", db = Postgres);

mod support;

use support::pg;

#[derive(Clone)]
struct AlwaysAuthProvider;

impl AuthProvider for AlwaysAuthProvider {
    type Error = cratestack::CratestackError;

    fn authenticate(
        &self,
        _request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        core::future::ready(Ok(CratestackContext::authenticated([(
            "id".to_owned(),
            Value::Int(1),
        )])))
    }
}

async fn reset_schema(pool: &cratestack::sqlx::PgPool) {
    for sql in [
        "DROP TABLE IF EXISTS cursor_notes",
        "DROP TABLE IF EXISTS plains",
        "CREATE TABLE cursor_notes (id BIGINT PRIMARY KEY, label TEXT NOT NULL, rank BIGINT, hidden BOOLEAN NOT NULL)",
        "CREATE TABLE plains (id BIGINT PRIMARY KEY, label TEXT NOT NULL)",
    ] {
        cratestack::sqlx::query(sql).execute(pool).await.expect(sql);
    }
}

async fn insert(
    pool: &cratestack::sqlx::PgPool,
    id: i64,
    label: &str,
    rank: Option<i64>,
    hidden: bool,
) {
    cratestack::sqlx::query(
        "INSERT INTO cursor_notes (id, label, rank, hidden) VALUES ($1, $2, $3, $4)",
    )
    .bind(id)
    .bind(label)
    .bind(rank)
    .bind(hidden)
    .execute(pool)
    .await
    .expect("insert note");
}

fn router(pool: &cratestack::sqlx::PgPool) -> cratestack::axum::Router {
    cratestack_schema::axum::model_router(
        cratestack_schema::Cratestack::builder(pool.clone()).build(),
        (),
        JsonCodec,
        AlwaysAuthProvider,
    )
}

async fn get(router: &cratestack::axum::Router, uri: &str) -> (StatusCode, Json) {
    let response = router
        .clone()
        .oneshot(
            Request::get(uri)
                .header("accept", "application/json")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("route request should complete");
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    (status, serde_json::from_slice(&body).expect("json body"))
}

fn ids(page: &Json) -> Vec<i64> {
    page["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|i| i["id"].as_i64().unwrap())
        .collect()
}

fn next(page: &Json) -> Option<String> {
    page["pageInfo"]["nextCursor"].as_str().map(str::to_owned)
}

fn q(cursor: &str, extra: &str) -> String {
    format!(
        "/cursor_notes?cursor={}&{extra}",
        cursor.replace('=', "%3D")
    )
}

#[tokio::test]
async fn cursor_paging_is_stable_across_inserts_and_honours_policy() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    // Sort `-rank` (DESC NULLS LAST) then id: 5(r9) 3(r7) 1(r7) 2(r1) 4(NULL) ; id 9 is hidden.
    insert(pool, 1, "a", Some(7), false).await;
    insert(pool, 2, "b", Some(1), false).await;
    insert(pool, 3, "c", Some(7), false).await;
    insert(pool, 4, "d", None, false).await;
    insert(pool, 5, "e", Some(9), false).await;
    insert(pool, 9, "h", Some(8), true).await;
    let router = router(pool);

    let (status, first) = get(&router, &q("", "limit=2&sort=-rank")).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(
        ids(&first),
        [5, 1],
        "tiebreak on id ascending within rank 7"
    );
    assert_eq!(first["totalCount"], 5, "hidden row never counted");
    let cursor = next(&first).expect("more rows remain");

    // Inserts while the client holds the cursor: one ahead of it (rank 10,
    // already-served territory), one tied on the cursor row's rank, one NULL,
    // one hidden that sorts inside the next page.
    insert(pool, 10, "j", Some(10), false).await;
    insert(pool, 11, "k", Some(7), false).await;
    insert(pool, 12, "l", None, false).await;
    insert(pool, 13, "m", Some(5), true).await;

    let (status, second) = get(&router, &q(&cursor, "limit=2&sort=-rank")).await;
    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(
        ids(&second),
        [3, 11],
        "ties after the cursor row, no repeat of 5/1, no 10"
    );
    let (_, third) = get(&router, &q(&next(&second).unwrap(), "limit=2&sort=-rank")).await;
    assert_eq!(ids(&third), [2, 4], "rank 1 then the first NULL");
    let (_, fourth) = get(&router, &q(&next(&third).unwrap(), "limit=2&sort=-rank")).await;
    assert_eq!(
        ids(&fourth),
        [12],
        "NULLS LAST tail; hidden rows 9/13 never appear"
    );
    assert_eq!(next(&fourth), None, "no nextCursor on the last page");
    assert_eq!(fourth["pageInfo"]["hasNextPage"], false);

    // No `sort`: primary key ascending.
    let (_, by_id) = get(&router, &q("", "limit=3")).await;
    assert_eq!(ids(&by_id), [1, 2, 3]);
}

#[tokio::test]
async fn cursor_refusals_are_bad_requests() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    for id in 1..=4 {
        insert(pool, id, "x", Some(id), false).await;
    }
    let router = router(pool);

    let (_, page) = get(&router, &q("", "limit=1&sort=rank")).await;
    let cursor = next(&page).expect("cursor");

    for (what, uri) in [
        ("different sort order", q(&cursor, "limit=1&sort=-rank")),
        (
            "cursor with offset",
            q(&cursor, "limit=1&sort=rank&offset=1"),
        ),
        ("garbage cursor", q("%%%not-a-cursor", "sort=rank")),
        (
            "oversized cursor",
            q(&"A".repeat(cratestack::MAX_CURSOR_LEN + 1), "sort=rank"),
        ),
        ("relation/unknown sort", q("", "sort=nope")),
        (
            "limit above the cap",
            q("", &format!("limit={}", cratestack::MAX_LIST_LIMIT + 1)),
        ),
        ("non-paged model", "/plains?cursor=".to_owned()),
    ] {
        let (status, body) = get(&router, &uri).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{what}: {body}");
    }
}
