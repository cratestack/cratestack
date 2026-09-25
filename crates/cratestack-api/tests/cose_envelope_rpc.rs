//! The envelope layer in front of a generated `transport rpc` router
//! (cratestack#1006; the REST twin is `cose_envelope_rest.rs`): the
//! `AuthProvider` sees the opened payload (D5), the handler's context
//! records the signer without authenticating (D2), `/rpc/batch` is one
//! unary message (D11), and the ADR 0006 §12 placement: envelope, then the
//! rate limiter, then the idempotency layer.

mod cose_support;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cose_support::*;
use cratestack::axum::Router;
use cratestack::axum::body::Body;
use cratestack::axum::http::{Request, StatusCode, header};
use cratestack::rpc::{RpcRequest, RpcResponseFrame};
use cratestack::{
    CratestackCodec, CratestackContext, CratestackError, VerifiedSigner, include_server_schema,
};
use cratestack_axum::idempotency::IdempotencyLayer;
use cratestack_axum::ratelimit::{RateLimitConfig, RateLimitLayer};
use cratestack_codec_cbor::CborCodec;

include_server_schema!("tests/fixtures/rpc_batch_no_database.cstack", db = None);

#[derive(Clone, Default)]
struct Procedures(Arc<AtomicUsize>);

impl cratestack_schema::procedures::ProcedureRegistry for Procedures {
    fn ping(
        &self,
        _db: &cratestack_schema::Cratestack,
        ctx: &CratestackContext,
        args: cratestack_schema::procedures::ping::Args,
        _authorized: cratestack_schema::procedures::ping::Authorized,
    ) -> impl core::future::Future<
        Output = Result<cratestack_schema::procedures::ping::Output, CratestackError>,
    > + Send {
        self.0.fetch_add(1, Ordering::SeqCst);
        let signer = ctx.verified_signer().map(VerifiedSigner::alg);
        async move {
            Ok(cratestack_schema::PingReply {
                nonce: format!("{}|signer={signer:?}", args.args.nonce),
            })
        }
    }
}

/// Authenticates everyone, and records the body and whether the envelope's
/// signer was visible to it.
#[derive(Clone, Default)]
struct RecordingAuth(Arc<Mutex<Vec<(Vec<u8>, bool)>>>);

impl cratestack::AuthProvider for RecordingAuth {
    type Error = CratestackError;

    fn authenticate(
        &self,
        request: &cratestack::RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        let signed = request.extensions.get::<VerifiedSigner>().is_some();
        self.0.lock().unwrap().push((request.body.to_vec(), signed));
        core::future::ready(Ok(CratestackContext::authenticated([(
            "id".to_owned(),
            cratestack::Value::Int(1),
        )])))
    }
}

fn generated(procedures: Procedures, auth: RecordingAuth) -> Router {
    cratestack_schema::axum::rpc_router(
        cratestack_schema::Cratestack::builder().build(),
        procedures,
        (),
        CborCodec,
        auth,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
}

fn envelope() -> cratestack::envelope_layer::EnvelopeLayer {
    envelope_layer(cratestack_schema::SCHEMA_SHA256_BYTES)
        .rpc("")
        .build()
        .expect("layer")
}

const PING: Call = Call {
    route: "procedure.ping",
    schema_sha: cratestack_schema::SCHEMA_SHA256_BYTES,
};

fn ping_payload(nonce: &str) -> Vec<u8> {
    CborCodec
        .encode(&serde_json::json!({ "args": { "nonce": nonce } }))
        .expect("encode")
}

#[tokio::test]
async fn a_signed_call_runs_with_the_opened_payload_and_a_recorded_signer() {
    let auth = RecordingAuth::default();
    let router = generated(Procedures::default(), auth.clone()).layer(envelope());
    let payload = ping_payload("n1");
    let (sealed, req) = PING.request("/rpc/procedure.ping", &payload, &[]).await;
    let answer = send(&router, req).await;

    assert_eq!(answer.status, StatusCode::OK);
    let reply: cratestack_schema::PingReply = CborCodec
        .decode(&PING.open(&sealed, &answer).await.expect("verifies"))
        .expect("decode");
    assert_eq!(
        reply.nonce, "n1|signer=Some(-19)",
        "D2: recorded on the context"
    );
    let seen = auth.0.lock().unwrap().clone();
    assert_eq!(
        seen,
        vec![(payload, true)],
        "D5: the AuthProvider sees the opened payload"
    );
}

#[tokio::test]
async fn the_batch_route_is_one_signed_message() {
    let router = generated(Procedures::default(), RecordingAuth::default()).layer(envelope());
    let frames = vec![RpcRequest {
        id: 1,
        op: "procedure.ping".to_owned(),
        input: serde_json::json!({ "args": { "nonce": "b1" } }),
        idem: None,
    }];
    let batch = Call {
        route: "batch",
        schema_sha: cratestack_schema::SCHEMA_SHA256_BYTES,
    };
    let (sealed, req) = batch
        .request(
            "/rpc/batch",
            &CborCodec.encode(&frames).expect("encode"),
            &[],
        )
        .await;
    let answer = send(&router, req).await;
    assert_eq!(answer.status, StatusCode::OK);
    let frames: Vec<RpcResponseFrame> = CborCodec
        .decode(&batch.open(&sealed, &answer).await.expect("verifies"))
        .expect("frames");
    assert_eq!(frames.len(), 1);
    assert!(frames[0].error.is_none(), "{:?}", frames[0].error);
}

/// ADR 0006 §12, in the order D12 prescribes.
fn placed(procedures: Procedures, spy: Arc<SpyRateLimit>, envelope: bool) -> Router {
    let router = generated(procedures, RecordingAuth::default())
        .layer(IdempotencyLayer::new(
            Arc::new(MemoryIdempotency::default()),
            Duration::from_secs(60),
        ))
        .layer(RateLimitLayer::new(spy, RateLimitConfig::new(3, 0.001)));
    if envelope {
        router.layer(self::envelope())
    } else {
        router
    }
}

#[tokio::test]
async fn placement_without_the_envelope_a_keyed_call_is_refused_412() {
    let spy = Arc::new(SpyRateLimit::default());
    let router = placed(Procedures::default(), spy, false);
    let req = Request::post("/rpc/procedure.ping")
        .header(header::CONTENT_TYPE, "application/cbor")
        .header("idempotency-key", "k1")
        .body(Body::from(ping_payload("n1")))
        .expect("request");
    let answer = send(&router, req).await;
    assert_eq!(answer.status, StatusCode::PRECONDITION_FAILED);
}

#[tokio::test]
async fn placement_with_the_envelope_the_signer_is_the_principal() {
    let (spy, procedures) = (Arc::new(SpyRateLimit::default()), Procedures::default());
    let router = placed(procedures.clone(), spy.clone(), true);
    let (sealed, req) = PING
        .request(
            "/rpc/procedure.ping",
            &ping_payload("n1"),
            &[("idempotency-key", "k1")],
        )
        .await;
    let answer = send(&router, req).await;
    assert_eq!(
        answer.status,
        StatusCode::OK,
        "no 412: no Authorization, no ConnectInfo"
    );
    PING.open(&sealed, &answer).await.expect("verifies");
    let keys = spy.keys.lock().unwrap().clone();
    assert_eq!(keys.len(), 1);
    assert!(
        keys[0].starts_with("princ:"),
        "charged to the signer: {keys:?}"
    );

    // A retry is re-sealed (new `cti`) under the same key: the stored
    // response is replayed, sealed afresh for the new request.
    let (resealed, retry) = PING
        .request(
            "/rpc/procedure.ping",
            &ping_payload("n1"),
            &[("idempotency-key", "k1")],
        )
        .await;
    let replay = send(&router, retry).await;
    assert_eq!(replay.status, StatusCode::OK);
    PING.open(&resealed, &replay)
        .await
        .expect("verifies against the retry");
    PING.open(&sealed, &replay)
        .await
        .expect_err("not against the original");
    assert_eq!(
        procedures.0.load(Ordering::SeqCst),
        1,
        "the handler ran once"
    );

    // Same key, different body: the idempotency layer's 422, signed.
    let (conflicting, req) = PING
        .request(
            "/rpc/procedure.ping",
            &ping_payload("n2"),
            &[("idempotency-key", "k1")],
        )
        .await;
    let conflict = send(&router, req).await;
    assert_eq!(conflict.status, StatusCode::UNPROCESSABLE_ENTITY);
    let body = PING
        .open(&conflicting, &conflict)
        .await
        .expect("a signed 422");
    assert_eq!(error_code(&body), "invalid_argument");

    // The bucket (burst 3) is spent: the rate limiter's 429, signed.
    let (throttled, req) = PING
        .request("/rpc/procedure.ping", &ping_payload("n3"), &[])
        .await;
    let answer = send(&router, req).await;
    assert_eq!(answer.status, StatusCode::TOO_MANY_REQUESTS);
    let body = PING.open(&throttled, &answer).await.expect("a signed 429");
    assert_eq!(error_code(&body), "resource_exhausted");
}
