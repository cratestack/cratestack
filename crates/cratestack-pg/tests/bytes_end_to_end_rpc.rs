//! cratestack#1425 on the RPC transport, against a real Postgres: a model's
//! `Bytes` columns, its `@computed` `Bytes` value and an included relation's
//! `Bytes` all reach the wire as CBOR byte strings (RFC 8949 major type 2),
//! on `get`, `list`, write and `/rpc/batch` responses, while the JSON body is unchanged.
//! The REST twin is `bytes_end_to_end.rs`. PG-backed: skips without
//! `CRATESTACK_TEST_DATABASE_URL`.

use cratestack::axum::Router;
use cratestack::axum::body::Body;
use cratestack::axum::http::{Request, StatusCode};
use cratestack::rpc::RpcFrameValue;
use cratestack::serde_json::{Value as Json, json};
use cratestack::{AuthProvider, CratestackCodec, CratestackContext, RequestContext, Value};
use cratestack::{CratestackError, include_server_schema};
use cratestack_codec_cbor::CborCodec;
use cratestack_codec_json::JsonCodec;
use tower::util::ServiceExt;

include_server_schema!("tests/fixtures/bytes_end_to_end_rpc.cstack", db = Postgres);

mod support;

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

#[derive(Clone)]
struct NoProcedures;

impl cratestack_schema::procedures::ProcedureRegistry for NoProcedures {}

/// `fingerprint` is the digest reversed, so the test can tell a computed
/// value from a stored one.
#[derive(Clone)]
struct Resolver;

impl cratestack_schema::ComputedFieldResolver for Resolver {
    fn resolve_bytes_rpc_doc_fingerprint(
        &self,
        _db: &cratestack_schema::Cratestack,
        source: &cratestack_schema::BytesRpcDoc,
        _ctx: &CratestackContext,
    ) -> impl core::future::Future<Output = Result<Vec<u8>, CratestackError>> + Send {
        let reversed: Vec<u8> = source.digest.iter().rev().copied().collect();
        async move { Ok(reversed) }
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

fn rpc(path: &str, accept: &str, body: &RpcFrameValue) -> Request<Body> {
    // Request and response use the same codec.
    let bytes = if accept == JsonCodec::CONTENT_TYPE {
        JsonCodec.encode(body).expect("encode")
    } else {
        CborCodec.encode(body).expect("encode")
    };
    Request::post(path)
        .header("accept", accept)
        .header("content-type", accept)
        .body(Body::from(bytes))
        .expect("request")
}

fn obj(entries: Vec<(&str, RpcFrameValue)>) -> RpcFrameValue {
    RpcFrameValue::Object(
        entries
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect(),
    )
}

fn num(n: i64) -> RpcFrameValue {
    RpcFrameValue::from(json!(n))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn frame(bytes: &[u8]) -> RpcFrameValue {
    CborCodec.decode(bytes).expect("a CBOR response")
}

fn field<'a>(value: &'a RpcFrameValue, key: &str) -> &'a RpcFrameValue {
    match value {
        RpcFrameValue::Object(map) => map.get(key).unwrap_or_else(|| panic!("no `{key}`")),
        other => panic!("expected an object, got {other:?}"),
    }
}

fn is_bytes(value: &RpcFrameValue, expected: &[u8]) -> bool {
    matches!(value, RpcFrameValue::Bytes(bytes) if bytes == expected)
}

#[tokio::test]
async fn model_bytes_reach_the_wire_as_byte_strings_over_rpc() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    for sql in [
        "DROP TABLE IF EXISTS bytes_rpc_parts, bytes_rpc_docs",
        "CREATE TABLE bytes_rpc_docs (id BIGINT PRIMARY KEY, digest BYTEA NOT NULL, signature BYTEA)",
        "CREATE TABLE bytes_rpc_parts (id BIGINT PRIMARY KEY, doc_id BIGINT NOT NULL, payload BYTEA NOT NULL)",
    ] {
        cratestack::sqlx::query(sql).execute(pool).await.expect(sql);
    }
    let build = |codec_is_json: bool| -> Router {
        let db = cratestack_schema::Cratestack::builder(pool.clone()).build();
        if codec_is_json {
            cratestack_schema::axum::rpc_router(
                db,
                NoProcedures,
                Resolver,
                JsonCodec,
                AlwaysAuthProvider,
                cratestack::DEFAULT_BODY_LIMIT_BYTES,
            )
        } else {
            cratestack_schema::axum::rpc_router(
                db,
                NoProcedures,
                Resolver,
                CborCodec,
                AlwaysAuthProvider,
                cratestack::DEFAULT_BODY_LIMIT_BYTES,
            )
        }
    };
    let router = build(false);
    let json_router = build(true);
    let cbor = CborCodec::CONTENT_TYPE;

    // create, `digest` sent as a byte string; the response carries them too.
    let (status, created) = send(
        &router,
        rpc(
            "/rpc/model.BytesRpcDoc.create",
            cbor,
            &obj(vec![
                ("id", num(1)),
                ("digest", RpcFrameValue::Bytes(vec![1, 2, 255])),
            ]),
        ),
    )
    .await;
    assert!(
        status.is_success(),
        "{status} {}",
        String::from_utf8_lossy(&created)
    );
    let created = frame(&created);
    assert!(
        is_bytes(field(&created, "digest"), &[1, 2, 255]),
        "{created:?}"
    );
    assert!(
        is_bytes(field(&created, "fingerprint"), &[255, 2, 1]),
        "{created:?}"
    );

    // a second doc through the legacy integer-array body.
    let (status, body) = send(
        &router,
        rpc(
            "/rpc/model.BytesRpcDoc.create",
            cbor,
            &RpcFrameValue::from(json!({"id": 2, "digest": [9], "signature": [7, 8]})),
        ),
    )
    .await;
    assert!(
        status.is_success(),
        "{status} {}",
        String::from_utf8_lossy(&body)
    );
    cratestack::sqlx::query(
        "INSERT INTO bytes_rpc_parts VALUES (10, 1, '\\x0a0b'), (11, 1, '\\x')",
    )
    .execute(pool)
    .await
    .unwrap();

    // get
    let (status, body) = send(
        &router,
        rpc(
            "/rpc/model.BytesRpcDoc.get",
            cbor,
            &obj(vec![("id", num(1))]),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let hex_body = hex(&body);
    assert!(
        hex_body.contains("66646967657374430102ff"),
        "digest: {hex_body}"
    );
    assert!(
        !hex_body.contains("83010218ff"),
        "no integer array: {hex_body}"
    );

    // list with the included to-many relation
    let (status, body) = send(
        &router,
        rpc(
            "/rpc/model.BytesRpcDoc.list",
            cbor,
            &RpcFrameValue::from(json!({"include": ["parts"], "sort": "id"})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let RpcFrameValue::Array(items) = frame(&body) else {
        panic!("expected a list")
    };
    assert_eq!(items.len(), 2);
    assert!(is_bytes(field(&items[0], "digest"), &[1, 2, 255]));
    assert!(is_bytes(field(&items[1], "signature"), &[7, 8]));
    let RpcFrameValue::Array(parts) = field(&items[0], "parts") else {
        panic!("expected parts")
    };
    let payloads: Vec<&RpcFrameValue> = parts.iter().map(|p| field(p, "payload")).collect();
    assert!(payloads.iter().any(|p| is_bytes(p, &[0x0a, 0x0b])));
    assert!(payloads.iter().any(|p| is_bytes(p, &[])));

    // `/rpc/batch`: every frame's output keeps its byte strings.
    let (status, body) = send(
        &router,
        rpc(
            "/rpc/batch",
            cbor,
            &RpcFrameValue::from(json!([
                {"id": 0, "op": "model.BytesRpcDoc.get", "input": {"id": 1}},
                {"id": 1, "op": "model.BytesRpcDoc.get", "input": {"id": 2}},
            ])),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let RpcFrameValue::Array(frames) = frame(&body) else {
        panic!("expected frames")
    };
    assert!(is_bytes(
        field(field(&frames[0], "output"), "digest"),
        &[1, 2, 255]
    ));
    assert!(is_bytes(
        field(field(&frames[1], "output"), "signature"),
        &[7, 8]
    ));

    // JSON stays an integer array.
    let (status, body) = send(
        &json_router,
        rpc(
            "/rpc/model.BytesRpcDoc.get",
            JsonCodec::CONTENT_TYPE,
            &obj(vec![("id", num(1))]),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let value: Json = cratestack::serde_json::from_slice(&body).unwrap();
    assert_eq!(value["digest"], json!([1, 2, 255]));
    assert_eq!(value["fingerprint"], json!([255, 2, 1]));
}
