//! cratestack#1352: `POST /rpc/batch` frames must carry `Uuid` / `Bytes` in
//! their codec forms, as the unary route does (#1312). Before the fix a frame's
//! `input` / `output` was a `serde_json::Value`, which has no byte-string arm,
//! so a CBOR batch could not carry either type. Real generated `rpc_router` on
//! a loopback listener, real generated RPC client, plus raw frames shaped the
//! way the TypeScript client writes them.

mod server {
    cratestack::include_server_schema!("tests/fixtures/cbor_binary_shapes_rpc.cstack", db = None);
}

mod client {
    cratestack::include_client_schema!("tests/fixtures/cbor_binary_shapes_rpc.cstack");
}

use cratestack::{CratestackCodec, CratestackContext, CratestackError};
use cratestack_client_rust::{
    CborCodec, ClientConfig, CratestackClient, HttpClientCodec, JsonCodec,
};
use serde::{Deserialize, Serialize};
use server::cratestack_schema as srv;

#[derive(Clone, Default)]
struct Procedures;

impl srv::procedures::ProcedureRegistry for Procedures {
    fn echo(
        &self,
        _db: &srv::Cratestack,
        _ctx: &CratestackContext,
        args: srv::procedures::echo::Args,
        _authorized: srv::procedures::echo::Authorized,
    ) -> impl core::future::Future<Output = Result<srv::procedures::echo::Output, CratestackError>> + Send
    {
        async move { Ok(args.input) }
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

const UUID: u128 = 0x0123_4567_89ab_cdef_0123_4567_89ab_cdef;

async fn spawn<C: CratestackCodec + HttpClientCodec>(
    codec: C,
) -> (
    client::cratestack_schema::client::Client<C>,
    std::net::SocketAddr,
    tokio::task::JoinHandle<()>,
) {
    let router = srv::axum::rpc_router(
        srv::Cratestack::builder().build(),
        Procedures,
        (),
        codec.clone(),
        AllowAllAuth,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        cratestack::axum::serve(listener, router).await.unwrap();
    });
    let _ = rustls::crypto::ring::default_provider().install_default();
    let base_url = reqwest::Url::parse(&format!("http://{addr}")).unwrap();
    let runtime = CratestackClient::new(ClientConfig::new(base_url), codec);
    (
        client::cratestack_schema::client::Client::new(runtime),
        addr,
        server,
    )
}

fn args() -> client::cratestack_schema::procedures::echo::Args {
    client::cratestack_schema::procedures::echo::Args {
        input: client::cratestack_schema::Blob {
            id: uuid::Uuid::from_u128(UUID),
            data: vec![0, 1, 2, 255],
        },
    }
}

#[tokio::test]
async fn batched_procedure_call_carries_uuid_and_bytes_in_cbor_form() {
    let (client, _addr, server) = spawn(CborCodec).await;
    let sent = args();
    let mut batch = client.batch();
    let handle = client.procedures().echo(&sent).queue(&mut batch);
    let mut results = batch.send().await.expect("the batch round trip");
    let reply = results.take(handle);
    server.abort();
    let reply = reply.expect("a batched call must send and receive Uuid/Bytes in CBOR form");
    assert_eq!(reply.id, sent.input.id);
    assert_eq!(reply.data, sent.input.data);
}

#[derive(Serialize)]
struct Frame<'a> {
    id: u64,
    op: &'a str,
    input: cratestack::Value,
}

#[derive(Deserialize)]
struct Blob {
    id: uuid::Uuid,
    data: Vec<u8>,
}

#[derive(Deserialize)]
struct OutFrame {
    id: u64,
    output: Option<Blob>,
    error: Option<cratestack::rpc::RpcErrorBody>,
}

/// The frame the TypeScript client writes: a native CBOR byte string inside
/// `input`, for both the `Uuid` (16 bytes) and the `Bytes` field.
#[tokio::test]
async fn ts_shaped_native_cbor_frame_round_trips() {
    use std::collections::BTreeMap;
    let (_client, addr, server) = spawn(CborCodec).await;
    let blob = BTreeMap::from([
        (
            "id".to_owned(),
            cratestack::Value::Bytes(UUID.to_be_bytes().to_vec()),
        ),
        (
            "data".to_owned(),
            cratestack::Value::Bytes(vec![0, 1, 2, 255]),
        ),
    ]);
    let input = cratestack::Value::Map(BTreeMap::from([(
        "input".to_owned(),
        cratestack::Value::Map(blob),
    )]));
    let body = CborCodec
        .encode(&vec![Frame {
            id: 0,
            op: "procedure.echo",
            input,
        }])
        .unwrap();
    let response = reqwest::Client::new()
        .post(format!("http://{addr}/rpc/batch"))
        .header("content-type", "application/cbor")
        .header("accept", "application/cbor")
        .body(body)
        .send()
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.bytes().await.unwrap();
    server.abort();
    assert_eq!(status, 200, "body: {}", String::from_utf8_lossy(&bytes));
    let frames: Vec<OutFrame> = CborCodec.decode(&bytes).unwrap();
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].id, 0);
    assert!(frames[0].error.is_none());
    let out = frames[0].output.as_ref().expect("an output");
    assert_eq!(out.id, uuid::Uuid::from_u128(UUID));
    assert_eq!(out.data, vec![0, 1, 2, 255]);
}

/// Regression guard: the JSON batch wire is unchanged byte for byte.
#[tokio::test]
async fn json_batch_is_byte_identical() {
    let (client, addr, server) = spawn(JsonCodec).await;
    let body = br#"[{"id":0,"op":"procedure.echo","input":{"input":{"id":"01234567-89ab-cdef-0123-456789abcdef","data":[0,1,2,255]}}}]"#;
    let response = reqwest::Client::new()
        .post(format!("http://{addr}/rpc/batch"))
        .header("content-type", "application/json")
        .header("accept", "application/json")
        .body(body.to_vec())
        .send()
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.bytes().await.unwrap();
    assert_eq!(status, 200);
    assert_eq!(
        String::from_utf8(bytes.to_vec()).unwrap(),
        r#"[{"id":0,"output":{"data":[0,1,2,255],"id":"01234567-89ab-cdef-0123-456789abcdef"}}]"#
    );

    let sent = args();
    let mut batch = client.batch();
    let handle = client.procedures().echo(&sent).queue(&mut batch);
    let mut results = batch.send().await.expect("the JSON batch round trip");
    let reply = results.take(handle);
    server.abort();
    let reply = reply.expect("a JSON batched call");
    assert_eq!(reply.id, sent.input.id);
    assert_eq!(reply.data, sent.input.data);
}
