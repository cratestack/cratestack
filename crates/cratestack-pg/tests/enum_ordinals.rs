//! Ordinal enums (`Pending = 1`, `docs/design/enum-ordinals.md`) end to
//! end on Postgres: the generated ORM against the emitter's own DDL, the
//! policies lowered to ordinal literals, and the REST surface, where a
//! body carries the number (a name still decodes) and a query string
//! carries the name.

use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::include_server_schema;
use cratestack::sqlx::{Row as _, query};
use cratestack::{
    AuthProvider, CratestackCodec, CratestackContext, CratestackError, RequestContext, Value,
};
use cratestack_codec_json::JsonCodec;
use tower::util::ServiceExt;

include_server_schema!("tests/fixtures/enum_ordinals.cstack", db = Postgres);

mod support;
use support::pg;

use cratestack_schema::{CreatePaymentInput, PaymentStatus, UpdatePaymentInput};

const SCHEMA_SRC: &str = include_str!("fixtures/enum_ordinals.cstack");

#[derive(Clone)]
struct AnyoneAuth;

impl AuthProvider for AnyoneAuth {
    type Error = CratestackError;

    fn authenticate(
        &self,
        _request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        core::future::ready(Ok(ctx()))
    }
}

fn ctx() -> CratestackContext {
    CratestackContext::authenticated([("id".to_owned(), Value::Int(1))])
}

async fn reset(pool: &cratestack::sqlx::PgPool) {
    query("DROP TABLE IF EXISTS cratestack_migrations, payments")
        .execute(pool)
        .await
        .expect("drop tables");
    let empty = cratestack_parser::parse_schema("").expect("empty schema parses");
    let next = cratestack_parser::parse_schema(SCHEMA_SRC).expect("fixture parses");
    let up = cratestack_migrate::emit::postgres::emit(
        &cratestack_migrate::diff(&empty, &next).expect("diff"),
    )
    .up;
    cratestack::apply_pending(
        pool,
        &[cratestack::Migration {
            id: "20261026000000_enum_ordinals".to_owned(),
            description: "ordinal enum fixture".to_owned(),
            up_pre: None,
            up,
            down: None,
        }],
    )
    .await
    .expect("emitter-generated DDL must apply cleanly");
}

async fn stored(pool: &cratestack::sqlx::PgPool, id: i64) -> (String, i16, Option<i16>, i16) {
    let row = query(
        "SELECT pg_typeof(status)::text, status, previous, initial FROM payments WHERE id = $1",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .expect("row stored");
    (row.get(0), row.get(1), row.get(2), row.get(3))
}

#[tokio::test]
async fn the_orm_stores_ordinals_and_the_policies_compare_them() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset(pool).await;
    let cool = cratestack_schema::Cratestack::builder(pool.clone()).build();

    let created = cool
        .payment()
        .create(CreatePaymentInput {
            id: 1,
            status: PaymentStatus::Approved,
            previous: Some(PaymentStatus::Pending),
        })
        .run(&ctx())
        .await
        .expect("create");
    assert_eq!(
        created.initial,
        PaymentStatus::Pending,
        "the column default"
    );
    assert_eq!(
        stored(pool, 1).await,
        ("smallint".to_owned(), 2, Some(1), 1)
    );

    // `status != Refunded` on create compares the input's ordinal.
    let refused = cool
        .payment()
        .create(CreatePaymentInput {
            id: 2,
            status: PaymentStatus::Refunded,
            previous: None,
        })
        .run(&ctx())
        .await;
    assert!(
        refused.is_err(),
        "a Refunded create must be denied: {refused:?}"
    );

    cool.payment()
        .create(CreatePaymentInput {
            id: 3,
            status: PaymentStatus::Pending,
            previous: None,
        })
        .run(&ctx())
        .await
        .expect("create a Pending row");
    // The read policy hides `Pending` (`status != Pending`) in SQL.
    let visible = cool.payment().find_many().run(&ctx()).await.expect("list");
    assert_eq!(visible.iter().map(|p| p.id).collect::<Vec<_>>(), [1]);

    let updated = cool
        .payment()
        .update(1)
        .set(UpdatePaymentInput {
            status: Some(PaymentStatus::Refunded),
            previous: Some(None),
            initial: None,
        })
        .run(&ctx())
        .await
        .expect("update");
    assert_eq!(updated.status, PaymentStatus::Refunded);
    assert_eq!(updated.previous, None);
    assert_eq!(stored(pool, 1).await.1, 5);
}

async fn call(router: &cratestack::axum::Router, request: Request<Body>) -> (StatusCode, String) {
    let response = router.clone().oneshot(request).await.expect("request");
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    (status, String::from_utf8(body.to_vec()).expect("utf8"))
}

fn post(body: &str) -> Request<Body> {
    Request::post("/payments")
        .header("content-type", JsonCodec::CONTENT_TYPE)
        .header("accept", JsonCodec::CONTENT_TYPE)
        .body(Body::from(body.to_owned()))
        .expect("request")
}

#[tokio::test]
async fn the_rest_wire_carries_the_number_and_the_query_string_the_name() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    reset(&test_pg.pool).await;
    let cool = cratestack_schema::Cratestack::builder(test_pg.pool.clone()).build();
    let router = cratestack_schema::axum::model_router(cool, (), JsonCodec, AnyoneAuth);

    let (status, body) = call(&router, post(r#"{"id":1,"status":2,"previous":null}"#)).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let row: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert_eq!(row["status"], serde_json::json!(2), "{body}");
    assert_eq!(row["initial"], serde_json::json!(1), "{body}");

    // A peer still sending names is decoded (§2.3); the reply is a number.
    let (status, body) = call(&router, post(r#"{"id":2,"status":"Refunded"}"#)).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "the create policy still applies: {body}"
    );
    let (status, body) = call(&router, post(r#"{"id":3,"status":"Approved"}"#)).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert!(body.contains(r#""status":2"#), "{body}");

    for bad in [r#"{"id":4,"status":3}"#, r#"{"id":4,"status":"Settled"}"#] {
        let (status, body) = call(&router, post(bad)).await;
        assert!(
            status.is_client_error(),
            "{bad} must be refused: {status} {body}"
        );
    }

    let list = |query: &str| {
        Request::get(format!("/payments?{query}"))
            .header("accept", JsonCodec::CONTENT_TYPE)
            .body(Body::empty())
            .expect("request")
    };
    let (status, body) = call(&router, list("status=Approved&sort=id")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let rows: Vec<serde_json::Value> = serde_json::from_str(&body).expect("json");
    assert_eq!(
        rows.iter().map(|r| r["id"].clone()).collect::<Vec<_>>(),
        [1, 3]
    );

    // Names, not numbers, in a query string (O2).
    let (status, _) = call(&router, list("status=2")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
