//! The generated REST routes of a model with `@@map` / `@map` (cratestack#1375):
//! the path stays `/map_banks` (not the `map_bank` table), query keys and
//! bodies use field names (`displayName`), and every statement behind a route
//! runs against the mapped table and columns.

use cratestack::axum::Router;
use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::include_server_schema;
use cratestack::{AuthProvider, CratestackContext, CratestackError, RequestContext, Value};
use cratestack_codec_json::JsonCodec;
use serde_json::{Value as Json, json};
use tower::util::ServiceExt;

include_server_schema!("tests/fixtures/storage_map.cstack", db = Postgres);

mod storage_map_support;
mod support;

use storage_map_support::{reset_schema, seed};
use support::pg;

#[derive(Clone)]
struct AlwaysAuthProvider;

impl AuthProvider for AlwaysAuthProvider {
    type Error = CratestackError;

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

async fn call(
    router: &Router,
    method: &str,
    uri: &str,
    body: Option<Json>,
    if_match: Option<&str>,
) -> (StatusCode, Json) {
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .header("accept", "application/json")
        .header("content-type", "application/json");
    if let Some(tag) = if_match {
        request = request.header("if-match", tag);
    }
    let body = body.map_or_else(Body::empty, |b| Body::from(b.to_string()));
    let response = router
        .clone()
        .oneshot(request.body(body).expect("request"))
        .await
        .expect("dispatch");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    (status, serde_json::from_slice(&bytes).unwrap_or(Json::Null))
}

fn ids(page: &Json) -> Vec<i64> {
    page["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|i| i["id"].as_i64().unwrap())
        .collect()
}

#[tokio::test]
async fn rest_routes_keep_their_paths_and_run_against_the_mapped_names() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;
    let router = cratestack_schema::axum::model_router(
        cratestack_schema::Cratestack::builder(pool.clone()).build(),
        (),
        JsonCodec,
        AlwaysAuthProvider,
    );

    // The table's name is not a route.
    let (status, _) = call(&router, "GET", "/map_bank", None, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Filter, sort and cursor use field names; the hidden row is policy-filtered.
    let (status, page) = call(&router, "GET", "/map_banks?sort=-displayName", None, None).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(ids(&page), [2, 1]);
    assert_eq!(page["totalCount"], 2);
    let (_, filtered) = call(&router, "GET", "/map_banks?region=us", None, None).await;
    assert_eq!(ids(&filtered), [2]);
    let (_, first) = call(
        &router,
        "GET",
        "/map_banks?cursor=&limit=1&sort=displayName",
        None,
        None,
    )
    .await;
    assert_eq!(ids(&first), [1]);
    let cursor = first["pageInfo"]["nextCursor"]
        .as_str()
        .expect("next cursor")
        .replace('=', "%3D");
    let (status, second) = call(
        &router,
        "GET",
        &format!("/map_banks?limit=1&sort=displayName&cursor={cursor}"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{second}");
    assert_eq!(ids(&second), [2]);

    let (status, with) = call(&router, "GET", "/map_banks/1?include=deposits", None, None).await;
    assert_eq!(status, StatusCode::OK, "{with}");
    assert_eq!(with["deposits"].as_array().map(Vec::len), Some(2), "{with}");
    assert_eq!(with["displayName"], "Alpha");

    // POST, PATCH with If-Match (mapped @version column), 409 on a mapped unique.
    let new = json!({"id": 4, "code": "DDD", "displayName": "Delta", "ssn": "s", "taxRef": "t", "region": "eu"});
    let (status, created) = call(&router, "POST", "/map_banks", Some(new.clone()), None).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let (status, _) = call(&router, "POST", "/map_banks", Some(json!({"id": 5, "code": "DDD", "displayName": "x", "ssn": "s", "taxRef": "t", "region": "eu"})), None).await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "duplicate on the mapped unique column"
    );
    let (status, patched) = call(
        &router,
        "PATCH",
        "/map_banks/4",
        Some(json!({"displayName": "Delta 2"})),
        Some("\"0\""),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{patched}");
    assert_eq!(patched["displayName"], "Delta 2");
    let (status, _) = call(
        &router,
        "PATCH",
        "/map_banks/4",
        Some(json!({"region": "us"})),
        Some("\"0\""),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::PRECONDITION_FAILED,
        "stale version after the bump"
    );
    let (status, _) = call(&router, "DELETE", "/map_banks/4", None, Some("\"1\"")).await;
    assert!(status.is_success(), "{status}");
}
