//! `@authn` on the RPC binding (ADR 0022, cratestack#1244), through the real
//! generated `rpc_router()` (CBOR): a unary call, and a batch whose frames
//! are judged one by one, so a frame that needs a step-up is refused while
//! its neighbours run. The REST half is `authn_rest.rs`.

mod authn_support;

use authn_support::{HeaderProvider, STALE_WEAK, STRONG, pay_details_all_unmet};
use cratestack::CratestackCodec;
use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::rpc::{RPC_BATCH_PATH, RpcErrorBody, RpcRequest, RpcResponseFrame};
use cratestack::{CratestackContext, CratestackError, include_server_schema};
use cratestack_codec_cbor::CborCodec;
use cratestack_schema::procedures::{balance, close, pay};
use tower::ServiceExt;

include_server_schema!("tests/fixtures/authn_rpc.cstack", db = None);

#[derive(Clone, Default)]
struct Procedures;

fn ok() -> cratestack_schema::Receipt {
    cratestack_schema::Receipt { ok: true }
}

impl cratestack_schema::procedures::ProcedureRegistry for Procedures {
    async fn pay(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        _args: pay::Args,
        _authorized: pay::Authorized,
    ) -> Result<pay::Output, CratestackError> {
        Ok(ok())
    }

    async fn close(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        _args: close::Args,
        _authorized: close::Authorized,
    ) -> Result<close::Output, CratestackError> {
        Ok(ok())
    }

    async fn balance(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        _args: balance::Args,
        _authorized: balance::Authorized,
    ) -> Result<balance::Output, CratestackError> {
        Ok(ok())
    }
}

async fn post(path: &str, body: Vec<u8>, caller: &[(&str, &str)]) -> (StatusCode, Vec<u8>) {
    let app = cratestack_schema::axum::rpc_router(
        cratestack_schema::Cratestack::builder().build(),
        Procedures,
        (),
        CborCodec,
        HeaderProvider,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    );
    let mut request = Request::post(path)
        .header("content-type", CborCodec::CONTENT_TYPE)
        .header("accept", CborCodec::CONTENT_TYPE);
    for (name, value) in caller {
        request = request.header(*name, *value);
    }
    let response = app
        .oneshot(request.body(Body::from(body)).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, bytes.to_vec())
}

fn args() -> Vec<u8> {
    CborCodec
        .encode(&serde_json::json!({ "args": { "amount": 5 } }))
        .unwrap()
}

#[tokio::test]
async fn a_unary_step_up_is_unauthenticated_with_the_member_and_details() {
    let (status, bytes) = post("/rpc/procedure.pay", args(), STALE_WEAK).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let body: RpcErrorBody = CborCodec.decode(&bytes).unwrap();
    assert_eq!(body.code, "unauthenticated");
    assert_eq!(body.message, "step-up authentication required");
    assert_eq!(body.extra["error"], "insufficient_user_authentication");
    assert_eq!(body.details, Some(pay_details_all_unmet()));

    let (status, _) = post("/rpc/procedure.pay", args(), STRONG).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn a_batch_judges_each_frame() {
    let frame = |id: u64, op: &str| RpcRequest {
        id,
        op: op.into(),
        input: serde_json::json!({ "args": { "amount": 5 } }),
        idem: None,
    };
    let batch = vec![
        frame(1, "procedure.balance"),
        frame(2, "procedure.pay"),
        frame(3, "procedure.close"),
    ];
    let (status, bytes) = post(
        RPC_BATCH_PATH,
        CborCodec.encode(&batch).unwrap(),
        STALE_WEAK,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let frames: Vec<RpcResponseFrame> = CborCodec.decode(&bytes).unwrap();
    assert!(frames[0].error.is_none(), "balance has no requirement");
    let step_up = frames[1].error.as_ref().expect("pay needs a step-up");
    assert_eq!(step_up.code, "unauthenticated");
    assert_eq!(step_up.extra["error"], "insufficient_user_authentication");
    assert_eq!(step_up.details, Some(pay_details_all_unmet()));
    let denied = frames[2].error.as_ref().expect("close is admin-only");
    assert_eq!(denied.code, "permission_denied", "@allow answers first");
}
