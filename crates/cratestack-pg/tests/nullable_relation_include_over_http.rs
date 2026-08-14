//! End-to-end HTTP coverage for a *null* to-one relation `include`,
//! over both wire codecs — closing a coverage gap in cratestack#430's
//! fix (`ProjectedValue`, `crates/cratestack-axum/src/projection.rs`).
//!
//! That PR removed a "strip `null` map entries" workaround from
//! `project_<model>_model_value` and claimed, as a documented side
//! effect, that it also fixed a *separate*, previously-uncovered bug on
//! nullable to-one relation includes: the old code always built
//! `serde_json::Value::Null` for a missing/denied relation
//! (`crates/cratestack-macros/src/relation/include_arm.rs`), and
//! `serde_json::Value::Null`'s own `Serialize` impl calls
//! `serializer.serialize_unit()` — which `minicbor-serde` encodes as a
//! CBOR *empty array*, not CBOR null, corrupting the field. Every
//! existing `include=` HTTP assertion in this test suite
//! (`policy_db.rs`) covers a relation that's actually *present* on the
//! wire; none of them exercised the null branch.
//!
//! `ProjectedValue::Null` now always calls `serialize_none()` instead
//! (see `crates/cratestack-axum/src/projection.rs`), which
//! `projection::tests` unit-tests in isolation — but not through the
//! real macro-generated relation-include codegen path, and not against
//! a real Postgres row. This test drives that full path: a genuinely
//! nullable FK (`ProjWidget.ownerId Int?`), `include=owner` over HTTP,
//! decoded via the *real* `CborCodec`/`JsonCodec`.

use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::include_server_schema;
use cratestack::sqlx::query;
use cratestack::{AuthProvider, CratestackContext, CratestackError, RequestContext, Value};
use cratestack_codec_cbor::CborCodec;
use cratestack_codec_json::JsonCodec;
use tower::util::ServiceExt;

include_server_schema!(
    "tests/fixtures/nullable_relation_projection.cstack",
    db = Postgres
);

mod support;

use support::pg;

async fn reset_schema(pool: &cratestack::sqlx::PgPool) {
    query("DROP TABLE IF EXISTS proj_widgets, proj_widget_owners")
        .execute(pool)
        .await
        .expect("drop tables");
    query("CREATE TABLE proj_widget_owners (id BIGINT PRIMARY KEY, name TEXT NOT NULL)")
        .execute(pool)
        .await
        .expect("create proj_widget_owners");
    query(
        "CREATE TABLE proj_widgets (
            id BIGINT PRIMARY KEY,
            label TEXT NOT NULL,
            owner_id BIGINT
        )",
    )
    .execute(pool)
    .await
    .expect("create proj_widgets");
}

async fn seed(pool: &cratestack::sqlx::PgPool) {
    query("INSERT INTO proj_widget_owners (id, name) VALUES (1, 'Alice')")
        .execute(pool)
        .await
        .expect("seed owner");
    query("INSERT INTO proj_widgets (id, label, owner_id) VALUES (1, 'owned', 1), (2, 'orphan', NULL)")
        .execute(pool)
        .await
        .expect("seed widgets");
}

#[derive(Clone)]
struct PassThroughAuth;

impl AuthProvider for PassThroughAuth {
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

/// Asserts, for the given codec, that `GET /proj_widgets?include=owner`:
/// - decodes at all (the old `Value::Null` -> `serialize_unit()` bug
///   made CBOR non-decodable here: `minicbor-serde` wrote an empty
///   array where a scalar-shaped map value was structurally expected
///   downstream),
/// - the owned widget's `owner` include is present and correct, and
/// - the orphan widget's `owner` include is an explicit wire null
///   (`Value::Null`), not a missing key and not an empty array/object.
async fn assert_null_include_round_trips<C>(pool: &cratestack::sqlx::PgPool, codec: C)
where
    C: cratestack::HttpTransport + Clone + cratestack::CratestackCodec,
{
    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let router = cratestack_schema::axum::model_router(cool, codec.clone(), PassThroughAuth);

    let response = router
        .oneshot(
            Request::get("/proj_widgets?include=owner&sort=id")
                .header("accept", C::CONTENT_TYPE)
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");
    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body should read");
    let widgets: Vec<cratestack::serde_json::Value> = codec
        .decode(&body)
        .expect("include=owner response must decode under this codec");

    let owned = widgets[0]
        .as_object()
        .expect("first widget should be an object");
    assert_eq!(
        owned.get("label"),
        Some(&cratestack::serde_json::Value::from("owned"))
    );
    let owner = owned
        .get("owner")
        .and_then(|value| value.as_object())
        .expect("owned widget's owner include should be a present object");
    assert_eq!(
        owner.get("name"),
        Some(&cratestack::serde_json::Value::from("Alice"))
    );

    let orphan = widgets[1]
        .as_object()
        .expect("second widget should be an object");
    assert_eq!(
        orphan.get("label"),
        Some(&cratestack::serde_json::Value::from("orphan"))
    );
    // The crux of this test: the key must be *present* and *exactly*
    // `Value::Null` — not absent (would make `.get` return `None`, a
    // different, harmless shape this test wouldn't distinguish from the
    // bug), and not any other non-null shape (e.g. the pre-fix CBOR
    // empty-array corruption, which `serde_json::Value`'s CBOR-aware
    // deserialize would surface as `Value::Array(vec![])` here, not
    // `Value::Null`).
    assert_eq!(
        orphan.get("owner"),
        Some(&cratestack::serde_json::Value::Null),
        "orphan widget's null owner include must round-trip as an explicit wire null, got: {:?}",
        orphan.get("owner"),
    );
}

#[tokio::test]
async fn nullable_to_one_relation_include_round_trips_as_null_over_cbor() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;

    assert_null_include_round_trips(pool, CborCodec).await;
}

#[tokio::test]
async fn nullable_to_one_relation_include_round_trips_as_null_over_json() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_schema(pool).await;
    seed(pool).await;

    assert_null_include_round_trips(pool, JsonCodec).await;
}
