//! cratestack#1359 on the REST transport (the Dart clients speak CBOR over
//! REST): create and update bodies carry `Uuid`s as CBOR *text*, against a
//! real Postgres; responses stay 16-byte. The RPC twin is
//! `uuid_end_to_end_rpc.rs`. PG-backed: skips without
//! `CRATESTACK_TEST_DATABASE_URL`.

use cratestack::axum::Router;
use cratestack::axum::body::Body;
use cratestack::axum::http::{Request, StatusCode};
use cratestack::serde_json::{Value as Json, json};
use cratestack::{AuthProvider, CratestackCodec, CratestackContext, RequestContext, Value};
use cratestack::{CratestackError, include_server_schema};
use cratestack_codec_cbor::CborCodec;
use tower::util::ServiceExt;

include_server_schema!("tests/fixtures/uuid_end_to_end.cstack", db = Postgres);

mod support;

use cratestack_schema::UuidRestDoc;
use support::pg;

const A: &str = "11111111-1111-4111-8111-111111111111";
const B: &str = "22222222-2222-4222-8222-222222222222";
const OWNER: &str = "33333333-3333-4333-8333-333333333333";

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

#[derive(Clone)]
struct Procedures;

impl cratestack_schema::procedures::ProcedureRegistry for Procedures {
    async fn find_uuid_rest_docs(
        &self,
        db: &cratestack_schema::Cratestack,
        ctx: &CratestackContext,
        args: cratestack_schema::procedures::find_uuid_rest_docs::Args,
        _authorized: cratestack_schema::procedures::find_uuid_rest_docs::Authorized,
    ) -> Result<cratestack_schema::procedures::find_uuid_rest_docs::Output, CratestackError> {
        cratestack_schema::build_uuid_rest_doc_query_from_find_many(db, &args.query)
            .order_by(cratestack_schema::uuid_rest_doc::title().asc())
            .run(ctx)
            .await
    }
}

async fn send(router: &Router, request: Request<Body>) -> (StatusCode, Vec<u8>) {
    let response = router.clone().oneshot(request).await.expect("dispatch");
    let status = response.status();
    let bytes = cratestack::axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    (status, bytes.to_vec())
}

fn cbor_request(builder: cratestack::axum::http::request::Builder, body: Json) -> Request<Body> {
    builder
        .header("accept", CborCodec::CONTENT_TYPE)
        .header("content-type", CborCodec::CONTENT_TYPE)
        .body(Body::from(CborCodec.encode(&body).expect("encode")))
        .expect("request")
}

#[tokio::test]
async fn text_uuids_work_in_rest_bodies_and_find_many_filters_against_postgres() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    for sql in [
        "DROP TABLE IF EXISTS uuid_rest_docs",
        "CREATE TABLE uuid_rest_docs (id UUID PRIMARY KEY, owner_id UUID NOT NULL, parent_id UUID, title TEXT NOT NULL)",
    ] {
        cratestack::sqlx::query(sql).execute(pool).await.expect(sql);
    }
    let router = cratestack_schema::axum::router(
        cratestack_schema::Cratestack::builder(pool.clone()).build(),
        Procedures,
        (),
        CborCodec,
        AlwaysAuthProvider,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    );

    let (status, body) = send(
        &router,
        cbor_request(
            Request::post("/uuid_rest_docs"),
            json!({"id": A, "ownerId": OWNER, "parentId": B, "title": "a"}),
        ),
    )
    .await;
    assert!(status.is_success(), "{status} {}", String::from_utf8_lossy(&body));
    let created: UuidRestDoc = CborCodec.decode(&body).expect("strict Uuid decode");
    assert_eq!(created.id.to_string(), A);

    let (status, body) = send(
        &router,
        cbor_request(
            Request::patch(format!("/uuid_rest_docs/{A}")),
            json!({"ownerId": B, "parentId": null}),
        ),
    )
    .await;
    assert!(status.is_success(), "{status} {}", String::from_utf8_lossy(&body));
    let updated: UuidRestDoc = CborCodec.decode(&body).expect("strict Uuid decode");
    assert_eq!(updated.ownerId.to_string(), B);
    assert_eq!(updated.parentId, None);

    let (status, body) = send(
        &router,
        cbor_request(
            Request::post("/$procs/findUuidRestDocs"),
            json!({"query": {"where": {"ownerId": {"eq": B}, "id": {"in": [A]}}}}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let found: Vec<UuidRestDoc> = CborCodec.decode(&body).expect("typed list");
    assert_eq!(found.len(), 1);

    let (status, _) = send(
        &router,
        cbor_request(
            Request::post("/uuid_rest_docs"),
            json!({"id": "nope", "ownerId": OWNER, "title": "x"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
