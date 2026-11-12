//! cratestack#1312: the generated Rust client's typed procedure calls must
//! put `Uuid` / `Bytes` on the wire in their CBOR forms (a 16-byte string
//! and a byte string), not the JSON shapes (text / integer array) that a
//! detour through `serde_json::Value` produces. Real generated
//! `rpc_router` on a loopback listener, real generated RPC client.

mod server {
    cratestack::include_server_schema!("tests/fixtures/cbor_binary_shapes_rpc.cstack", db = None);
}

mod client {
    cratestack::include_client_schema!("tests/fixtures/cbor_binary_shapes_rpc.cstack");
}

use cratestack::{CratestackContext, CratestackError};
use cratestack_client_rust::{CborCodec, ClientConfig, CratestackClient};
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

async fn spawn() -> (
    client::cratestack_schema::client::Client<CborCodec>,
    tokio::task::JoinHandle<()>,
) {
    let router = srv::axum::rpc_router(
        srv::Cratestack::builder().build(),
        Procedures,
        (),
        cratestack_codec_cbor::CborCodec,
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
    let runtime = CratestackClient::new(ClientConfig::new(base_url), CborCodec);
    (
        client::cratestack_schema::client::Client::new(runtime),
        server,
    )
}

fn args() -> client::cratestack_schema::procedures::echo::Args {
    client::cratestack_schema::procedures::echo::Args {
        input: client::cratestack_schema::Blob {
            id: uuid::Uuid::from_u128(0x0123_4567_89ab_cdef_0123_4567_89ab_cdef),
            data: vec![0, 1, 2, 255],
        },
    }
}

#[tokio::test]
async fn typed_procedure_call_carries_uuid_and_bytes_in_cbor_form() {
    let (client, server) = spawn().await;
    let sent = args();
    let reply = client.procedures().echo(&sent).await;
    server.abort();
    let reply = reply.expect("a typed call must send Uuid/Bytes in their CBOR forms");
    assert_eq!(reply.id, sent.input.id);
    assert_eq!(reply.data, sent.input.data);
}

// The batched path (`.queue(&mut batch)`) is covered in
// `cbor_binary_shapes_rpc_batch.rs` (cratestack#1352).
