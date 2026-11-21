//! cratestack#1425 on the REST transport, against a real Postgres: a model's
//! `Bytes` columns, its `@computed` `Bytes` value and an included relation's
//! `Bytes` all reach the wire as CBOR byte strings (RFC 8949 major type 2),
//! on `get`, `list` and write responses, while the JSON body is unchanged.
//! The RPC twin is `bytes_end_to_end_rpc.rs`. PG-backed: skips without
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

include_server_schema!("tests/fixtures/bytes_end_to_end_rest.cstack", db = Postgres);

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

/// `fingerprint` is the digest reversed, so the test can tell a computed
/// value from a stored one.
#[derive(Clone)]
struct Resolver;

impl cratestack_schema::ComputedFieldResolver for Resolver {
    fn resolve_bytes_rest_doc_fingerprint(
        &self,
        _db: &cratestack_schema::Cratestack,
        source: &cratestack_schema::BytesRestDoc,
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

fn request(
    builder: cratestack::axum::http::request::Builder,
    codec_type: &str,
    body: Option<Vec<u8>>,
) -> Request<Body> {
    builder
        .header("accept", codec_type)
        .header("content-type", codec_type)
        .body(Body::from(body.unwrap_or_default()))
        .expect("request")
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
async fn model_bytes_reach_the_wire_as_byte_strings_and_json_is_unchanged() {
    let _guard = pg::serial_guard().await;
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    for sql in [
        "DROP TABLE IF EXISTS bytes_rest_parts, bytes_rest_docs",
        "CREATE TABLE bytes_rest_docs (id BIGINT PRIMARY KEY, digest BYTEA NOT NULL, signature BYTEA)",
        "CREATE TABLE bytes_rest_parts (id BIGINT PRIMARY KEY, doc_id BIGINT NOT NULL, payload BYTEA NOT NULL)",
    ] {
        cratestack::sqlx::query(sql).execute(pool).await.expect(sql);
    }
    let router = cratestack_schema::axum::model_router(
        cratestack_schema::Cratestack::builder(pool.clone()).build(),
        Resolver,
        CborCodec,
        AlwaysAuthProvider,
    );
    let json_router = cratestack_schema::axum::model_router(
        cratestack_schema::Cratestack::builder(pool.clone()).build(),
        Resolver,
        JsonCodec,
        AlwaysAuthProvider,
    );
    let cbor = CborCodec::CONTENT_TYPE;
    let json_type = JsonCodec::CONTENT_TYPE;

    // Create, sending `digest` as a byte string: the write response carries
    // byte strings too.
    let body = CborCodec
        .encode(&RpcFrameValue::Object(
            [
                ("id".to_owned(), RpcFrameValue::from(json!(1))),
                ("digest".to_owned(), RpcFrameValue::Bytes(vec![1, 2, 255])),
            ]
            .into(),
        ))
        .unwrap();
    let (status, created) = send(
        &router,
        request(Request::post("/bytes_rest_docs"), cbor, Some(body)),
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
    assert!(matches!(field(&created, "signature"), RpcFrameValue::Null));

    // A second doc through the legacy array body, with a nullable value.
    let (status, body) = send(
        &router,
        request(
            Request::post("/bytes_rest_docs"),
            cbor,
            Some(
                CborCodec
                    .encode(&json!({"id": 2, "digest": [9], "signature": [7, 8]}))
                    .unwrap(),
            ),
        ),
    )
    .await;
    assert!(
        status.is_success(),
        "{status} {}",
        String::from_utf8_lossy(&body)
    );
    cratestack::sqlx::query(
        "INSERT INTO bytes_rest_parts VALUES (10, 1, '\\x0a0b'), (11, 1, '\\x')",
    )
    .execute(pool)
    .await
    .unwrap();

    // get: raw bytes carry the major-type-2 head for each field.
    let (status, body) = send(
        &router,
        request(Request::get("/bytes_rest_docs/1"), cbor, None),
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
    let doc = frame(&body);
    assert!(is_bytes(field(&doc, "fingerprint"), &[255, 2, 1]));

    // list, with the included to-many relation.
    let (status, body) = send(
        &router,
        request(
            Request::get("/bytes_rest_docs?include=parts&sort=id"),
            cbor,
            None,
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

    // The same GET as JSON: byte-identical to the pre-#1425 body shape.
    let (status, body) = send(
        &json_router,
        request(Request::get("/bytes_rest_docs/1"), json_type, None),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let value: Json = cratestack::serde_json::from_slice(&body).unwrap();
    assert_eq!(value["digest"], json!([1, 2, 255]));
    assert_eq!(value["fingerprint"], json!([255, 2, 1]));
}
