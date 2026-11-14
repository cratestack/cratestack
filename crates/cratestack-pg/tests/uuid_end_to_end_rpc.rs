//! cratestack#1359: `Uuid` as CBOR *text* against a real Postgres, over the
//! RPC transport. The bodies are `CborCodec.encode(&json!(..))`, exactly what
//! `@cratestack/cbor` and the Dart bridge put on the wire for a string. Every
//! response must still carry the 16-byte form, which the typed decode below
//! (strict `uuid::Uuid`) proves. PG-backed: skips without
//! `CRATESTACK_TEST_DATABASE_URL`.

use cratestack::axum::Router;
use cratestack::axum::body::Body;
use cratestack::axum::http::{Request, StatusCode};
use cratestack::serde_json::{Value as Json, json};
use cratestack::{AuthProvider, CratestackCodec, CratestackContext, RequestContext, Value};
use cratestack::{CratestackError, include_server_schema};
use cratestack_codec_cbor::CborCodec;
use tower::util::ServiceExt;

include_server_schema!("tests/fixtures/uuid_end_to_end_rpc.cstack", db = Postgres);

mod support;

use cratestack_schema::UuidRpcDoc;
use cratestack_schema::procedures::find_uuid_docs;
use support::pg;

const A: &str = "11111111-1111-4111-8111-111111111111";
const B: &str = "22222222-2222-4222-8222-222222222222";
const OWNER: &str = "33333333-3333-4333-8333-333333333333";
const OTHER_OWNER: &str = "44444444-4444-4444-8444-444444444444";

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
    async fn find_uuid_docs(
        &self,
        db: &cratestack_schema::Cratestack,
        ctx: &CratestackContext,
        args: find_uuid_docs::Args,
        _authorized: find_uuid_docs::Authorized,
    ) -> Result<find_uuid_docs::Output, CratestackError> {
        cratestack_schema::build_uuid_rpc_doc_query_from_find_many(db, &args.query)
            .order_by(cratestack_schema::uuid_rpc_doc::title().asc())
            .run(ctx)
            .await
    }
}

async fn post(router: &Router, path: &str, body: Json) -> (StatusCode, Vec<u8>) {
    let response = router
        .clone()
        .oneshot(
            Request::post(path)
                .header("accept", CborCodec::CONTENT_TYPE)
                .header("content-type", CborCodec::CONTENT_TYPE)
                .body(Body::from(CborCodec.encode(&body).expect("encode")))
                .expect("request"),
        )
        .await
        .expect("dispatch");
    let status = response.status();
    let bytes = cratestack::axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    (status, bytes.to_vec())
}

fn doc(bytes: &[u8]) -> UuidRpcDoc {
    CborCodec
        .decode(bytes)
        .expect("a strict Uuid decode of the response")
}

#[tokio::test]
async fn text_uuids_work_in_every_rpc_request_position_against_postgres() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    for sql in [
        "DROP TABLE IF EXISTS uuid_rpc_docs",
        "CREATE TABLE uuid_rpc_docs (id UUID PRIMARY KEY, owner_id UUID NOT NULL, parent_id UUID, title TEXT NOT NULL)",
    ] {
        cratestack::sqlx::query(sql).execute(pool).await.expect(sql);
    }
    let router = cratestack_schema::axum::rpc_router(
        cratestack_schema::Cratestack::builder(pool.clone()).build(),
        Procedures,
        (),
        CborCodec,
        AlwaysAuthProvider,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    );

    // create: a text id, a text foreign key and a text nullable one.
    let (status, body) = post(
        &router,
        "/rpc/model.UuidRpcDoc.create",
        json!({"id": A, "ownerId": OWNER, "parentId": B, "title": "a"}),
    )
    .await;
    assert!(status.is_success(), "{}", String::from_utf8_lossy(&body));
    let created = doc(&body);
    assert_eq!(created.id.to_string(), A);
    assert_eq!(created.parentId.map(|u| u.to_string()).as_deref(), Some(B));

    let (status, body) = post(
        &router,
        "/rpc/model.UuidRpcDoc.create",
        json!({"id": B, "ownerId": OTHER_OWNER, "title": "b"}),
    )
    .await;
    assert!(status.is_success(), "{}", String::from_utf8_lossy(&body));

    // get / update / delete frames: `{ id }` as text.
    let (status, body) = post(&router, "/rpc/model.UuidRpcDoc.get", json!({"id": A})).await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    assert_eq!(doc(&body).title, "a");

    let (status, body) = post(
        &router,
        "/rpc/model.UuidRpcDoc.update",
        json!({"id": A, "patch": {"ownerId": OTHER_OWNER, "parentId": null, "title": "a2"}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let updated = doc(&body);
    assert_eq!(updated.ownerId.to_string(), OTHER_OWNER);
    assert_eq!(updated.parentId, None);

    // FindMany filters: eq and in, operands as text.
    let (status, body) = post(
        &router,
        "/rpc/procedure.findUuidDocs",
        json!({"query": {"where": {"ownerId": {"eq": OTHER_OWNER}, "id": {"in": [A, B]}}}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let found: Vec<UuidRpcDoc> = CborCodec.decode(&body).expect("typed list");
    assert_eq!(found.len(), 2, "both rows now belong to OTHER_OWNER");
    let (status, body) = post(
        &router,
        "/rpc/procedure.findUuidDocs",
        json!({"query": {"where": {"ownerId": {"eq": OWNER}}}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        CborCodec
            .decode::<Vec<UuidRpcDoc>>(&body)
            .unwrap()
            .is_empty()
    );

    // /rpc/batch with text ids in every frame.
    let (status, body) = post(
        &router,
        "/rpc/batch",
        json!([
            {"id": 0, "op": "model.UuidRpcDoc.get", "input": {"id": B}},
            {"id": 1, "op": "model.UuidRpcDoc.update", "input": {"id": B, "patch": {"ownerId": OWNER}}},
            {"id": 2, "op": "model.UuidRpcDoc.delete", "input": {"id": A}},
        ]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let frames: Json = CborCodec
        .decode::<Value>(&body)
        .map(|v| cratestack::serde_json::to_value(v).unwrap())
        .unwrap();
    for frame in frames.as_array().expect("frames") {
        assert!(frame.get("error").is_none_or(Json::is_null), "{frame}");
    }

    let (remaining,): (i64,) = cratestack::sqlx::query_as("SELECT count(*) FROM uuid_rpc_docs")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(remaining, 1, "delete by text id removed row A");

    // A malformed id is a 400, not a 500.
    let (status, _) = post(&router, "/rpc/model.UuidRpcDoc.get", json!({"id": "nope"})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
