//! The envelope layer in front of a generated REST router (cratestack#1006;
//! the RPC twin is `cose_envelope_rpc.rs`, and the two must stay in step,
//! per the transport-parity rule): D2, D5, and the ADR 0006 §12 placement.

mod cose_support;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cose_support::*;
use cratestack::axum::Router;
use cratestack::axum::body::Body;
use cratestack::axum::http::{Request, StatusCode, header};
use cratestack::{
    CratestackCodec, CratestackContext, CratestackError, VerifiedSigner, include_server_schema,
};
use cratestack_axum::idempotency::IdempotencyLayer;
use cratestack_axum::ratelimit::{RateLimitConfig, RateLimitLayer};
use cratestack_codec_cbor::CborCodec;

include_server_schema!("tests/fixtures/no_database_procedures.cstack", db = None);

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
                echo: format!("{}|signer={signer:?}", args.args.message),
            })
        }
    }
}

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
    cratestack_schema::axum::router(
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
        .rest("", cratestack_schema::axum::ROUTE_TRANSPORTS)
        .build()
        .expect("layer")
}

const PING: Call = Call {
    route: "/$procs/ping",
    schema_sha: cratestack_schema::SCHEMA_SHA256_BYTES,
};

fn ping_payload(message: &str) -> Vec<u8> {
    CborCodec
        .encode(&serde_json::json!({ "args": { "message": message } }))
        .expect("encode")
}

#[tokio::test]
async fn a_signed_call_runs_with_the_opened_payload_and_a_recorded_signer() {
    let auth = RecordingAuth::default();
    let router = generated(Procedures::default(), auth.clone()).layer(envelope());
    let payload = ping_payload("m1");
    let (sealed, req) = PING.request("/$procs/ping", &payload, &[]).await;
    let answer = send(&router, req).await;

    assert_eq!(answer.status, StatusCode::OK);
    let reply: cratestack_schema::PingReply = CborCodec
        .decode(&PING.open(&sealed, &answer).await.expect("verifies"))
        .expect("decode");
    assert_eq!(reply.echo, "m1|signer=Some(-19)");
    assert_eq!(auth.0.lock().unwrap().clone(), vec![(payload, true)]);
}

#[tokio::test]
async fn an_unsigned_call_is_refused_before_the_handler() {
    let procedures = Procedures::default();
    let router = generated(procedures.clone(), RecordingAuth::default()).layer(envelope());
    let req = Request::post("/$procs/ping")
        .header(header::CONTENT_TYPE, "application/cbor")
        .body(Body::from(ping_payload("m1")))
        .expect("request");
    let answer = send(&router, req).await;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED);
    assert!(!answer.is_sealed());
    assert_eq!(error_code(&answer.body), "UNAUTHORIZED");
    assert_eq!(procedures.0.load(Ordering::SeqCst), 0);
}

fn placed(procedures: Procedures, spy: Arc<SpyRateLimit>, envelope: bool) -> Router {
    let router = generated(procedures, RecordingAuth::default())
        .layer(IdempotencyLayer::new(
            Arc::new(MemoryIdempotency::default()),
            Duration::from_secs(60),
        ))
        .layer(RateLimitLayer::new(spy, RateLimitConfig::new(2, 0.001)));
    if envelope {
        router.layer(self::envelope())
    } else {
        router
    }
}

#[tokio::test]
async fn placement_without_the_envelope_a_keyed_call_is_refused_412() {
    let router = placed(
        Procedures::default(),
        Arc::new(SpyRateLimit::default()),
        false,
    );
    let req = Request::post("/$procs/ping")
        .header(header::CONTENT_TYPE, "application/cbor")
        .header("idempotency-key", "k1")
        .body(Body::from(ping_payload("m1")))
        .expect("request");
    assert_eq!(
        send(&router, req).await.status,
        StatusCode::PRECONDITION_FAILED
    );
}

#[tokio::test]
async fn placement_with_the_envelope_the_signer_is_the_principal() {
    let (spy, procedures) = (Arc::new(SpyRateLimit::default()), Procedures::default());
    let router = placed(procedures.clone(), spy.clone(), true);
    let key = [("idempotency-key", "k1")];
    let (sealed, req) = PING
        .request("/$procs/ping", &ping_payload("m1"), &key)
        .await;
    let answer = send(&router, req).await;
    assert_eq!(
        answer.status,
        StatusCode::OK,
        "no 412: no Authorization, no ConnectInfo"
    );
    PING.open(&sealed, &answer).await.expect("verifies");
    let keys = spy.keys.lock().unwrap().clone();
    assert!(keys.len() == 1 && keys[0].starts_with("princ:"), "{keys:?}");

    let (resealed, retry) = PING
        .request("/$procs/ping", &ping_payload("m1"), &key)
        .await;
    let replay = send(&router, retry).await;
    PING.open(&resealed, &replay)
        .await
        .expect("the stored answer, sealed for the retry");
    assert_eq!(procedures.0.load(Ordering::SeqCst), 1);

    let (throttled, req) = PING.request("/$procs/ping", &ping_payload("m2"), &[]).await;
    let answer = send(&router, req).await;
    assert_eq!(answer.status, StatusCode::TOO_MANY_REQUESTS);
    let body = PING.open(&throttled, &answer).await.expect("a signed 429");
    assert_eq!(error_code(&body), "TOO_MANY_REQUESTS");
}
