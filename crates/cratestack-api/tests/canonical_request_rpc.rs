//! ADR 0021 §3 (cratestack#1243) on the RPC binding: the provider sees
//! `POST /rpc/<op_id>` or `POST /rpc/batch`, never the prefix the router is
//! mounted under nor the internal `/$procs/...` path, with no query and the
//! exact body bytes the client sent.

mod canonical_support;

use canonical_support::{Recorder, Seen};
use cratestack::CratestackCodec;
use cratestack::axum::Router;
use cratestack::axum::body::Body;
use cratestack::axum::http::Request;
use cratestack::rpc::RpcRequest;
use cratestack::{CratestackContext, CratestackError, include_server_schema};
use cratestack_codec_cbor::CborCodec;
use tower::ServiceExt;

include_server_schema!("tests/fixtures/rpc_batch_no_database.cstack", db = None);

#[derive(Clone, Default)]
struct Procedures;

impl cratestack_schema::procedures::ProcedureRegistry for Procedures {
    async fn ping(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        args: cratestack_schema::procedures::ping::Args,
        _authorized: cratestack_schema::procedures::ping::Authorized,
    ) -> Result<cratestack_schema::procedures::ping::Output, CratestackError> {
        Ok(cratestack_schema::PingReply {
            nonce: args.args.nonce,
        })
    }
}

/// Posts `body` to `/api/v1<path>?trace=1` on a router nested under
/// `/api/v1`; returns what the provider saw.
async fn seen(path: &str, body: Vec<u8>) -> Seen {
    let recorder = Recorder::default();
    let rpc = cratestack_schema::axum::rpc_router(
        cratestack_schema::Cratestack::builder().build(),
        Procedures,
        (),
        CborCodec,
        recorder.clone(),
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    );
    let app = Router::new().nest("/api/v1", rpc);
    let request = Request::post(format!("/api/v1{path}?trace=1"))
        .header("content-type", CborCodec::CONTENT_TYPE)
        .header("accept", CborCodec::CONTENT_TYPE)
        .body(Body::from(body))
        .unwrap();
    let _ = app.oneshot(request).await.unwrap();
    recorder.only()
}

#[tokio::test]
async fn a_unary_call_is_its_op_route_and_the_wire_bytes() {
    let body = CborCodec
        .encode(&cratestack_schema::procedures::ping::Args {
            args: cratestack_schema::PingArgs { nonce: "n".into() },
        })
        .unwrap();
    assert_eq!(
        seen("/rpc/procedure.ping", body.clone()).await,
        Seen {
            method: "POST".into(),
            path: "/rpc/procedure.ping".into(),
            query: None,
            body,
        }
    );
}

#[tokio::test]
async fn a_batch_is_authenticated_once_as_the_whole_batch_body() {
    let frame = |id: u64| RpcRequest {
        id,
        op: "procedure.ping".into(),
        input: serde_json::json!({ "args": { "nonce": id.to_string() } }),
        idem: None,
    };
    let body = CborCodec.encode(&vec![frame(1), frame(2)]).unwrap();
    assert_eq!(
        seen("/rpc/batch", body.clone()).await,
        Seen {
            method: "POST".into(),
            path: "/rpc/batch".into(),
            query: None,
            body,
        }
    );
}
