//! The #1006 security and API reviews' findings over generated routers,
//! each the inverse of a reviewer's probe (seen failing before the fixes):
//! a batch cannot carry a `Required` op unsigned (B1), `/rpc/%62atch` is
//! never bound as `batch` (B2), a stripped `Idempotency-Key` does not
//! verify (S1), and an `@api_version` procedure is refused unsigned (the
//! API review's B1). The generated `envelope_layer` convenience builds
//! every layer here, so it is exercised on both transports' schemas.

mod cose_support;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use cose_support::*;
use cratestack::axum::Router;
use cratestack::axum::body::Body;
use cratestack::axum::http::{Request, StatusCode, header};
use cratestack::envelope_layer::{EnvelopeMode, PolicyRequest};
use cratestack::rpc::RpcRequest;
use cratestack::{CratestackCodec, CratestackContext, CratestackError};
use cratestack_axum::idempotency::IdempotencyLayer;
use cratestack_codec_cbor::CborCodec;

mod rpc {
    cratestack::include_server_schema!("tests/fixtures/rpc_batch_no_database.cstack", db = None);
}
mod versioned {
    cratestack::include_server_schema!("tests/fixtures/api_version.cstack", db = None);
}

#[derive(Clone, Default)]
struct Procedures(Arc<AtomicUsize>);

impl rpc::cratestack_schema::procedures::ProcedureRegistry for Procedures {
    fn ping(
        &self,
        _db: &rpc::cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        args: rpc::cratestack_schema::procedures::ping::Args,
        _authorized: rpc::cratestack_schema::procedures::ping::Authorized,
    ) -> impl core::future::Future<
        Output = Result<rpc::cratestack_schema::procedures::ping::Output, CratestackError>,
    > + Send {
        self.0.fetch_add(1, Ordering::SeqCst);
        async move {
            Ok(rpc::cratestack_schema::PingReply {
                nonce: args.args.nonce,
            })
        }
    }
}

fn everyone(_: &cratestack::axum::http::HeaderMap) -> Result<CratestackContext, CratestackError> {
    Ok(CratestackContext::authenticated([(
        "id".to_owned(),
        cratestack::Value::Int(1),
    )]))
}

fn rpc_router(procedures: Procedures) -> Router {
    rpc::cratestack_schema::axum::rpc_router(
        rpc::cratestack_schema::Cratestack::builder().build(),
        procedures,
        (),
        CborCodec,
        everyone,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
}

fn batch_body(nonce: &str) -> Vec<u8> {
    let frames = vec![RpcRequest {
        id: 1,
        op: "procedure.ping".to_owned(),
        input: serde_json::json!({ "args": { "nonce": nonce } }),
        idem: None,
    }];
    CborCodec.encode(&frames).expect("encode")
}

#[tokio::test]
async fn b1_a_batch_cannot_run_a_required_op_unsigned() {
    let procedures = Procedures::default();
    let policy = |request: &PolicyRequest<'_>| {
        if request.op() == "procedure.ping" {
            EnvelopeMode::Required
        } else {
            EnvelopeMode::Optional
        }
    };
    let layer = rpc::cratestack_schema::axum::envelope_layer(server_envelope(), policy, AUDIENCE)
        .build()
        .expect("layer");
    let router = rpc_router(procedures.clone()).layer(layer);
    let batch = send(
        &router,
        Request::post("/rpc/batch")
            .header(header::CONTENT_TYPE, "application/cbor")
            .body(Body::from(batch_body("b1")))
            .unwrap(),
    )
    .await;
    assert_eq!(batch.status, StatusCode::UNAUTHORIZED);
    assert_eq!(procedures.0.load(Ordering::SeqCst), 0, "ping ran unsigned");
}

#[tokio::test]
async fn b2_percent_encoded_batch_over_the_generated_router() {
    let procedures = Procedures::default();
    let router = rpc_router(procedures.clone()).layer(rpc_required());
    let batch = Call {
        route: "batch",
        schema_sha: rpc::cratestack_schema::SCHEMA_SHA256_BYTES,
    };
    let (sealed, req) = batch.request("/rpc/%62atch", &batch_body("b2"), &[]).await;
    let answer = send(&router, req).await;
    assert_eq!(answer.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert!(!answer.is_sealed());
    assert!(batch.open(&sealed, &answer).await.is_err());
    assert_eq!(procedures.0.load(Ordering::SeqCst), 0);
}

fn rpc_required() -> cratestack::envelope_layer::EnvelopeLayer {
    rpc::cratestack_schema::axum::envelope_layer(
        server_envelope(),
        EnvelopeMode::Required,
        AUDIENCE,
    )
    .build()
    .expect("layer")
}

fn ping_payload(nonce: &str) -> Vec<u8> {
    CborCodec
        .encode(&serde_json::json!({ "args": { "nonce": nonce } }))
        .expect("encode")
}

/// S1 over the real idempotency layer: a client re-seals its retry under
/// the same key; an on-path party that strips the key (so the retry would
/// run again instead of replaying) breaks the signature.
#[tokio::test]
async fn s1_a_stripped_idempotency_key_does_not_verify() {
    let procedures = Procedures::default();
    let router = rpc_router(procedures.clone())
        .layer(IdempotencyLayer::new(
            Arc::new(MemoryIdempotency::default()),
            Duration::from_secs(60),
        ))
        .layer(rpc_required());
    let ping = Call {
        route: "procedure.ping",
        schema_sha: rpc::cratestack_schema::SCHEMA_SHA256_BYTES,
    };
    let keyed = [("idempotency-key", "k1")];
    let (sent, first) = ping
        .request("/rpc/procedure.ping", &ping_payload("pay"), &keyed)
        .await;
    let first = send(&router, first).await;
    assert_eq!(first.status, StatusCode::OK);
    ping.open(&sent, &first).await.expect("bound to its key");

    let (_, mut stripped) = ping
        .request("/rpc/procedure.ping", &ping_payload("pay"), &keyed)
        .await;
    stripped.headers_mut().remove("idempotency-key");
    let stripped = send(&router, stripped).await;
    assert_eq!(stripped.status, StatusCode::UNAUTHORIZED);
    assert!(!stripped.is_sealed());

    let (sent, retry) = ping
        .request("/rpc/procedure.ping", &ping_payload("pay"), &keyed)
        .await;
    let retry = send(&router, retry).await;
    assert_eq!(retry.status, StatusCode::OK);
    ping.open(&sent, &retry)
        .await
        .expect("the stored answer, sealed for the retry");
    assert_eq!(procedures.0.load(Ordering::SeqCst), 1, "ran once");
}

#[derive(Clone, Default)]
struct Versioned;

impl versioned::cratestack_schema::procedures::ProcedureRegistry for Versioned {
    fn ping(
        &self,
        _db: &versioned::cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        args: versioned::cratestack_schema::procedures::ping::Args,
        _authorized: versioned::cratestack_schema::procedures::ping::Authorized,
    ) -> impl core::future::Future<
        Output = Result<versioned::cratestack_schema::procedures::ping::Output, CratestackError>,
    > + Send {
        async move {
            Ok(versioned::cratestack_schema::PingReply {
                echo: args.args.message,
            })
        }
    }

    fn plain(
        &self,
        db: &versioned::cratestack_schema::Cratestack,
        ctx: &CratestackContext,
        args: versioned::cratestack_schema::procedures::plain::Args,
        _authorized: versioned::cratestack_schema::procedures::plain::Authorized,
    ) -> impl core::future::Future<
        Output = Result<versioned::cratestack_schema::procedures::plain::Output, CratestackError>,
    > + Send {
        let _ = (db, ctx);
        async move {
            Ok(versioned::cratestack_schema::PingReply {
                echo: args.args.message,
            })
        }
    }
}

/// API review B1: an `@api_version` route under `Required` is refused
/// unsigned (the descriptor names the versioned path since #1079).
#[tokio::test]
async fn api_b1_a_versioned_procedure_is_not_reachable_unsigned() {
    let layer = versioned::cratestack_schema::axum::envelope_layer(
        server_envelope(),
        EnvelopeMode::Required,
        AUDIENCE,
    )
    .build()
    .expect("layer");
    let router = versioned::cratestack_schema::axum::router(
        versioned::cratestack_schema::Cratestack::builder().build(),
        Versioned,
        (),
        CborCodec,
        everyone,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
    .layer(layer);
    let body = CborCodec
        .encode(&serde_json::json!({ "args": { "message": "hi" } }))
        .expect("encode");
    let answer = send(
        &router,
        Request::post("/v2/$procs/ping")
            .header(header::CONTENT_TYPE, "application/cbor")
            .body(Body::from(body))
            .unwrap(),
    )
    .await;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED);
}
