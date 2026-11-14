//! cratestack#1359: a `Uuid` is 16 bytes on a CBOR response, but a
//! TypeScript or Dart client holds it as a string and sends it as CBOR
//! *text* (`@cratestack/cbor` and the Dart bridge carry JSON-shaped values).
//! The server must accept either form on every request position, over unary
//! RPC and `/rpc/batch`, and must leave the JSON wire byte for byte alone.
//!
//! Request bodies here are `CborCodec.encode(&serde_json::json!(..))`, which
//! is exactly what those two clients put on the wire for a string.

mod server {
    cratestack::include_server_schema!("tests/fixtures/cbor_uuid_shapes_rpc.cstack", db = None);
}

use cratestack::{CratestackCodec, CratestackContext, CratestackError};
use cratestack_client_rust::{CborCodec, JsonCodec};
use serde_json::json;
use server::cratestack_schema as srv;

const TEXT: &str = "01234567-89ab-cdef-0123-456789abcdef";
const UUID: u128 = 0x0123_4567_89ab_cdef_0123_4567_89ab_cdef;

#[derive(Clone, Default)]
struct Procedures;

impl srv::procedures::ProcedureRegistry for Procedures {
    async fn echo(
        &self,
        _db: &srv::Cratestack,
        _ctx: &CratestackContext,
        args: srv::procedures::echo::Args,
        _authorized: srv::procedures::echo::Authorized,
    ) -> Result<srv::procedures::echo::Output, CratestackError> {
        Ok(args.input)
    }

    async fn echo_id(
        &self,
        _db: &srv::Cratestack,
        _ctx: &CratestackContext,
        args: srv::procedures::echo_id::Args,
        _authorized: srv::procedures::echo_id::Authorized,
    ) -> Result<srv::procedures::echo_id::Output, CratestackError> {
        Ok(args.id)
    }
}

#[derive(Clone)]
struct AllowAllAuth;

impl cratestack::AuthProvider for AllowAllAuth {
    type Error = CratestackError;

    fn authenticate(
        &self,
        _request: &cratestack::RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        core::future::ready(Ok(CratestackContext::authenticated([(
            "id".to_owned(),
            cratestack::Value::Int(1),
        )])))
    }
}

async fn spawn<C: CratestackCodec>(codec: C) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    let router = srv::axum::rpc_router(
        srv::Cratestack::builder().build(),
        Procedures,
        (),
        codec,
        AllowAllAuth,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        cratestack::axum::serve(listener, router).await.unwrap();
    });
    (addr, server)
}

async fn post(addr: std::net::SocketAddr, path: &str, mime: &str, body: Vec<u8>) -> (u16, Vec<u8>) {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let response = reqwest::Client::new()
        .post(format!("http://{addr}{path}"))
        .header("content-type", mime)
        .header("accept", mime)
        .body(body)
        .send()
        .await
        .unwrap();
    let status = response.status().as_u16();
    (status, response.bytes().await.unwrap().to_vec())
}

fn cbor(value: serde_json::Value) -> Vec<u8> {
    CborCodec.encode(&value).unwrap()
}

/// `{"id": h'..'}` by hand: map(1), text(2) "id", then a byte string of
/// `payload.len()` (< 24) bytes — the form the server itself writes.
fn id_bytes_frame(payload: &[u8]) -> Vec<u8> {
    let mut out = vec![0xa1, 0x62, b'i', b'd', 0x40 | payload.len() as u8];
    out.extend_from_slice(payload);
    out
}

/// The `Item` a client sends, with every `Uuid` position as the given form.
fn item(id: serde_json::Value, ids: serde_json::Value) -> serde_json::Value {
    json!({"id": id, "maybe": ids[0], "ids": ids, "inner": {"ids": ids}})
}

/// Re-reads a CBOR response, asserting every `Uuid` is the 16-byte form.
fn assert_uuid_bytes(body: &[u8]) {
    let value: cratestack::Value = CborCodec.decode(body).unwrap();
    let text = format!("{value:?}");
    assert!(!text.contains(TEXT), "response must carry bytes, not text: {text}");
    assert!(text.contains("Bytes"), "{text}");
}

#[tokio::test]
async fn unary_accepts_text_uuids_in_every_position_and_still_answers_bytes() {
    let (addr, server) = spawn(CborCodec).await;
    let input = item(json!(TEXT), json!([TEXT, TEXT]));
    let (status, body) = post(addr, "/rpc/procedure.echo", "application/cbor", cbor(json!({"input": input}))).await;
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
    assert_uuid_bytes(&body);

    let (status, body) = post(addr, "/rpc/procedure.echoId", "application/cbor", cbor(json!({"id": TEXT}))).await;
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
    assert_eq!(body, [&[0x50u8][..], &UUID.to_be_bytes()[..]].concat(), "a bare Uuid return is 0x50 + 16 bytes");
    server.abort();
}

#[tokio::test]
async fn unary_still_accepts_the_bytes_form() {
    let (addr, server) = spawn(CborCodec).await;
    let body = id_bytes_frame(&UUID.to_be_bytes());
    let (status, reply) = post(addr, "/rpc/procedure.echoId", "application/cbor", body).await;
    server.abort();
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&reply));
    assert_eq!(reply, [&[0x50u8][..], &UUID.to_be_bytes()[..]].concat());
}

#[tokio::test]
async fn batch_accepts_text_uuids() {
    let (addr, server) = spawn(CborCodec).await;
    let frames = json!([
        {"id": 0, "op": "procedure.echo", "input": {"input": item(json!(TEXT), json!([TEXT]))}},
        {"id": 1, "op": "procedure.echoId", "input": {"id": TEXT}},
    ]);
    let (status, body) = post(addr, "/rpc/batch", "application/cbor", cbor(frames)).await;
    server.abort();
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&body));
    let out: Vec<serde_json::Value> = serde_json::from_str(
        &serde_json::to_string(&CborCodec.decode::<cratestack::Value>(&body).unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!(out.len(), 2);
    for frame in &out {
        assert!(frame.get("error").is_none_or(|e| e.is_null()), "{frame}");
    }
}

#[tokio::test]
async fn malformed_uuids_are_a_400_invalid_argument() {
    let (addr, server) = spawn(CborCodec).await;
    let (status, body) = post(addr, "/rpc/procedure.echoId", "application/cbor", cbor(json!({"id": "not-a-uuid"}))).await;
    assert_eq!(status, 400, "{}", String::from_utf8_lossy(&body));
    let short = id_bytes_frame(&[1; 15]);
    let (status, body) = post(addr, "/rpc/procedure.echoId", "application/cbor", short).await;
    server.abort();
    assert_eq!(status, 400, "{}", String::from_utf8_lossy(&body));
}

/// Regression guard: the JSON wire is unchanged, error text included.
#[tokio::test]
async fn json_wire_is_byte_identical() {
    let (addr, server) = spawn(JsonCodec).await;
    let body = format!(r#"{{"id":"{TEXT}"}}"#).into_bytes();
    let (status, reply) = post(addr, "/rpc/procedure.echoId", "application/json", body).await;
    assert_eq!(status, 200);
    assert_eq!(String::from_utf8(reply).unwrap(), format!(r#""{TEXT}""#));

    let (status, _) = post(addr, "/rpc/procedure.echoId", "application/json", br#"{"id":[1,2,3]}"#.to_vec()).await;
    assert_eq!(status, 400, "a JSON integer array stays refused");
    let (status, bad) = post(addr, "/rpc/procedure.echoId", "application/json", br#"{"id":"nope"}"#.to_vec()).await;
    server.abort();
    assert_eq!(status, 400);
    // The response is the generic envelope, as it was before this change.
    assert_eq!(
        String::from_utf8_lossy(&bad),
        r#"{"code":"invalid_argument","message":"invalid request payload"}"#
    );
}
