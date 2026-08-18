//! Regression test for the `/rpc/batch` signature-verification bug
//! reported against a real deployment: a correctly-signed batch request
//! was rejected with `signature content hash mismatch`, while the exact
//! same signing implementation was accepted on the unary `POST
//! /rpc/{op_id}` route.
//!
//! Root cause: `rpc_batch_dispatch` re-entered `rpc_dispatch_inner` once
//! per queued frame, and each per-op dispatch function (generated from
//! `crates/cratestack-macros/src/transport/rpc.rs`) independently calls
//! `AuthProvider::authenticate` against a `CanonicalRequest` built from
//! `/rpc/<op_id>` and that op's own (re-encoded) frame body — correct for
//! the real unary route those functions were written for, but NOT the
//! request a batch client actually sent or signed. A batch client signs
//! exactly one request: `POST /rpc/batch` with the raw, untouched bytes
//! of the whole frame sequence as the body
//! (`docs/design/rpc-transport.md` §5). Any `AuthProvider` whose verdict
//! is bound to the real request bytes it is given (a body-hash-bound
//! request-signing scheme, e.g.) can never match a fabricated per-op
//! identity that was never what was signed — independent of whether the
//! bytes happen to re-encode identically, because the *path* alone
//! (`/rpc/<op_id>` vs `/rpc/batch`) never matches. Re-invoking
//! `authenticate()` per frame is additionally incompatible with any
//! provider that treats a successful authentication as consuming a
//! single-use nonce, since one client-issued nonce would be claimed once
//! per frame instead of once per request.
//!
//! This test proves the fix directly: for a real multi-frame batch
//! dispatched through the actual generated `rpc_router`, `authenticate()`
//! must be called *exactly once*, and that one call must see the real
//! envelope identity — method `POST`, path `RPC_BATCH_PATH`, and a body
//! that is byte-for-byte the raw request body this test sent, not a
//! reconstruction of it.

use std::sync::{Arc, Mutex};

use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::rpc::{RPC_BATCH_PATH, RpcRequest, RpcResponseFrame};
use cratestack::{CratestackCodec, CratestackContext, CratestackError, include_server_schema};
use cratestack_codec_cbor::CborCodec;
use tower::ServiceExt;

include_server_schema!("tests/fixtures/rpc_batch_no_database.cstack", db = None);

#[derive(Clone, Default)]
struct Procedures;

impl cratestack_schema::procedures::ProcedureRegistry for Procedures {
    fn ping(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        args: cratestack_schema::procedures::ping::Args,
        _authorized: cratestack_schema::procedures::ping::Authorized,
    ) -> impl core::future::Future<
        Output = Result<cratestack_schema::procedures::ping::Output, CratestackError>,
    > + Send {
        async move {
            Ok(cratestack_schema::PingReply {
                nonce: args.args.nonce,
            })
        }
    }
}

/// One observed `authenticate()` call: exactly what a body-hash-bound
/// signing scheme (`vaam-store/platform`'s `auth-kit::SignedRequestVerifier`,
/// for instance) would hash and compare against a client-declared digest.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ObservedRequest {
    method: String,
    path: String,
    body: Vec<u8>,
}

/// Records every `RequestContext` it is asked to authenticate against,
/// then always succeeds — this test cares about *what identity* dispatch
/// hands the provider, not about rejecting anything.
#[derive(Clone, Default)]
struct RecordingAuthProvider {
    observed: Arc<Mutex<Vec<ObservedRequest>>>,
}

impl cratestack::AuthProvider for RecordingAuthProvider {
    type Error = CratestackError;

    fn authenticate(
        &self,
        request: &cratestack::RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        self.observed.lock().unwrap().push(ObservedRequest {
            method: request.method.to_owned(),
            path: request.path.to_owned(),
            body: request.body.to_vec(),
        });
        core::future::ready(Ok(CratestackContext::authenticated([(
            "id".to_owned(),
            cratestack::Value::Int(1),
        )])))
    }
}

fn build_router(auth: RecordingAuthProvider) -> cratestack::axum::Router {
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

#[tokio::test]
async fn batch_authenticates_the_real_envelope_exactly_once() {
    let auth = RecordingAuthProvider::default();
    let router = build_router(auth.clone());

    // The exact bytes this test sends on the wire — this IS what a real
    // client signs (and what its declared content hash covers) for a
    // batch call, per `docs/design/rpc-transport.md` §5.
    let frames = vec![ping_frame(1), ping_frame(2), ping_frame(3)];
    let raw_body = CborCodec.encode(&frames).expect("batch body should encode");

    let response = router
        .oneshot(
            Request::post(RPC_BATCH_PATH)
                .header("content-type", CborCodec::CONTENT_TYPE)
                .header("accept", CborCodec::CONTENT_TYPE)
                .body(Body::from(raw_body.clone()))
                .expect("request should build"),
        )
        .await
        .expect("batch dispatch should complete");

    assert_eq!(response.status(), StatusCode::OK);
    let response_bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body should buffer");
    let results: Vec<RpcResponseFrame> = CborCodec
        .decode(&response_bytes)
        .expect("batch response should decode");
    assert_eq!(results.len(), 3);
    for result in &results {
        assert!(
            result.error.is_none(),
            "every frame should have authenticated and dispatched successfully: {result:?}",
        );
    }

    // The decisive assertions. Before the fix these failed two ways:
    // `observed.len()` was 3 (one fabricated call per frame, each
    // claiming a single-use nonce on a real signing `AuthProvider` would
    // have rejected frames 2 and 3 as replayed even if the identity
    // matched), and every one of those three calls carried
    // `path == "/rpc/procedure.ping"` and `body == <that one frame's
    // re-encoded input>` — never the real `/rpc/batch` request this test
    // actually sent.
    let observed = auth.observed.lock().unwrap();
    assert_eq!(
        observed.len(),
        1,
        "authenticate() must run exactly once per batch request, not once per frame: {observed:?}",
    );
    let call = &observed[0];
    assert_eq!(call.method, "POST");
    assert_eq!(
        call.path, RPC_BATCH_PATH,
        "the authenticated identity must be the real /rpc/batch request, \
         not a per-op /rpc/<op_id> reconstruction",
    );
    assert_eq!(
        call.body, raw_body,
        "the authenticated body must be byte-for-byte the raw request body \
         this test sent — the same bytes a real client's declared content \
         hash was computed over — not a re-encoding of one frame's input",
    );
}

#[tokio::test]
async fn a_single_frame_batch_also_authenticates_the_batch_envelope_not_the_frame() {
    // Even a one-frame batch must be authenticated as `/rpc/batch`, not
    // silently treated like a unary `/rpc/<op_id>` call — the two routes
    // are never interchangeable for a body/path-bound `AuthProvider`,
    // regardless of frame count.
    let auth = RecordingAuthProvider::default();
    let router = build_router(auth.clone());

    let frames = vec![ping_frame(1)];
    let raw_body = CborCodec.encode(&frames).expect("batch body should encode");

    let response = router
        .oneshot(
            Request::post(RPC_BATCH_PATH)
                .header("content-type", CborCodec::CONTENT_TYPE)
                .header("accept", CborCodec::CONTENT_TYPE)
                .body(Body::from(raw_body.clone()))
                .expect("request should build"),
        )
        .await
        .expect("batch dispatch should complete");
    assert_eq!(response.status(), StatusCode::OK);

    let observed = auth.observed.lock().unwrap();
    assert_eq!(observed.len(), 1);
    assert_eq!(observed[0].path, RPC_BATCH_PATH);
    assert_eq!(observed[0].body, raw_body);
}
