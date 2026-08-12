//! cratestack#413 — `/rpc/batch` frame cap. Proves an over-limit batch is
//! rejected *before* dispatching a single frame — the compounding case the
//! ticket is actually about (an unbounded frame count multiplying the full
//! `authenticate()` + policy + dispatch cost `rpc_dispatch_inner` pays for
//! unary calls, once per frame). Runs against `cratestack-api`
//! (`db = None`), matching `docs/design/request-response-size-bounds.md`'s
//! test plan: rejection happens at the frame-count check inside
//! `rpc_batch_dispatch` itself, before any `authenticate()` call, so no
//! database is needed to prove it — and none of this crate's dependency
//! graph has one to begin with.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::include_server_schema;
use cratestack::rpc::{RPC_BATCH_PATH, RpcRequest, RpcResponseFrame};
use cratestack::{BATCH_MAX_ITEMS, CoolCodec, CoolContext, CoolError};
use cratestack_codec_cbor::CborCodec;
use tower::ServiceExt;

include_server_schema!("tests/fixtures/rpc_batch_no_database.cstack", db = None);

#[derive(Clone, Default)]
struct Procedures;

impl cratestack_schema::procedures::ProcedureRegistry for Procedures {
    fn ping(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CoolContext,
        args: cratestack_schema::procedures::ping::Args,
        _authorized: cratestack_schema::procedures::ping::Authorized,
    ) -> impl core::future::Future<
        Output = Result<cratestack_schema::procedures::ping::Output, CoolError>,
    > + Send {
        async move {
            Ok(cratestack_schema::PingReply {
                nonce: args.args.nonce,
            })
        }
    }
}

/// Counts real `authenticate()` invocations — the thing that must stay at
/// zero when a batch is rejected for being oversized. Always authenticates
/// successfully so an under-limit batch's frames dispatch normally.
#[derive(Clone, Default)]
struct SpyAuthProvider {
    calls: Arc<AtomicUsize>,
}

impl cratestack::AuthProvider for SpyAuthProvider {
    type Error = CoolError;

    fn authenticate(
        &self,
        _request: &cratestack::RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CoolContext, Self::Error>> + Send {
        self.calls.fetch_add(1, Ordering::SeqCst);
        core::future::ready(Ok(CoolContext::authenticated([(
            "id".to_owned(),
            cratestack::Value::Int(1),
        )])))
    }
}

fn build_router(auth: SpyAuthProvider) -> cratestack::axum::Router {
    let db = cratestack_schema::Cratestack::builder().build();
    cratestack_schema::axum::rpc_router(
        db,
        Procedures,
        CborCodec,
        auth,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
}

fn ping_frame(id: u64) -> RpcRequest {
    RpcRequest {
        id,
        op: "procedure.ping".into(),
        input: serde_json::json!({ "args": { "nonce": id.to_string() } }),
        idem: None,
    }
}

async fn post_batch(
    router: cratestack::axum::Router,
    frames: Vec<RpcRequest>,
) -> (StatusCode, Vec<u8>) {
    let body = CborCodec.encode(&frames).expect("batch body should encode");
    let response = router
        .oneshot(
            Request::post(RPC_BATCH_PATH)
                .header("content-type", CborCodec::CONTENT_TYPE)
                .body(Body::from(body))
                .expect("request should build"),
        )
        .await
        .expect("batch request should succeed at the transport level");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body should buffer");
    (status, bytes.to_vec())
}

#[tokio::test]
async fn over_limit_batch_is_rejected_before_any_frame_dispatches() {
    let auth = SpyAuthProvider::default();
    let router = build_router(auth.clone());

    let frames: Vec<RpcRequest> = (0..(BATCH_MAX_ITEMS as u64 + 1)).map(ping_frame).collect();
    let (status, body) = post_batch(router, frames).await;

    // Validation errors on the RPC dispatch path map to 422 — see
    // `CoolError::status_code`. Matches `cratestack-sqlx`'s /
    // `cratestack-rusqlite`'s own batch-size guard, which also raises
    // `CoolError::Validation`.
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let error: cratestack::rpc::RpcErrorBody =
        CborCodec.decode(&body).expect("error body should decode");
    assert!(
        error
            .message
            .contains(&format!("exceeds maximum of {BATCH_MAX_ITEMS}")),
        "error message should name the ceiling: {error:?}",
    );

    // The whole point: not one frame's `authenticate()` ran. If the cap
    // were checked after the loop started (or not at all), this would be
    // `BATCH_MAX_ITEMS + 1`, not `0`.
    assert_eq!(
        auth.calls.load(Ordering::SeqCst),
        0,
        "an over-limit batch must be rejected before dispatching any frame",
    );
}

#[tokio::test]
async fn under_limit_batch_dispatches_every_frame_normally() {
    let auth = SpyAuthProvider::default();
    let router = build_router(auth.clone());

    let frames: Vec<RpcRequest> = (0..3u64).map(ping_frame).collect();
    let (status, body) = post_batch(router, frames).await;

    assert_eq!(status, StatusCode::OK);
    let responses: Vec<RpcResponseFrame> = CborCodec.decode(&body).expect("batch should decode");
    assert_eq!(responses.len(), 3);
    for response in &responses {
        assert!(
            response.error.is_none(),
            "frame should succeed: {response:?}"
        );
    }

    // The frame cap must not have collateral effects on a legitimately
    // small batch — every frame's `authenticate()` still ran.
    assert_eq!(auth.calls.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn batch_at_exactly_the_ceiling_is_accepted() {
    let auth = SpyAuthProvider::default();
    let router = build_router(auth.clone());

    let frames: Vec<RpcRequest> = (0..(BATCH_MAX_ITEMS as u64)).map(ping_frame).collect();
    let (status, body) = post_batch(router, frames).await;

    assert_eq!(status, StatusCode::OK);
    let responses: Vec<RpcResponseFrame> = CborCodec.decode(&body).expect("batch should decode");
    assert_eq!(responses.len(), BATCH_MAX_ITEMS);
    assert_eq!(auth.calls.load(Ordering::SeqCst), BATCH_MAX_ITEMS);
}
