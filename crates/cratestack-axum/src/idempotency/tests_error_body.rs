//! cratestack#846, scope extension: the idempotency middleware had the
//! same opaque-body defect as the rate-limit layer. An idempotency-key
//! conflict is a routine, expected outcome that a client is *supposed* to
//! branch on, and it used to arrive as `text/plain` — undecodable by any
//! generated client.
//!
//! Deliberately NOT covered here, because it does not exist: a fail-open
//! policy. A failed idempotency store must keep failing the request.

#![cfg(test)]

use std::sync::Arc;
use std::time::Duration;

use axum::body::{Body, to_bytes};
use axum::extract::Request;
use axum::response::Response;
use cratestack_codec_cbor::CborCodec;
use cratestack_core::rpc::RpcErrorBody;
use cratestack_core::{CratestackCodec, CratestackErrorResponse};
use http::StatusCode;
use tower::{Layer, Service};

use super::layer::IdempotencyLayer;
use super::store::IdempotencyStore;
use super::tests_stream_bypass::InMemoryIdempotencyStore;

fn request(uri: &str, key: &str, body: &'static str) -> Request {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("idempotency-key", key)
        .header("authorization", "Bearer test")
        .body(Body::from(body))
        .unwrap()
}

fn ok_handler(_req: Request) -> OkFuture {
    Box::pin(async { Ok(Response::new(Body::from("ok"))) })
}

type OkFuture = std::pin::Pin<
    Box<dyn std::future::Future<Output = Result<Response, std::convert::Infallible>> + Send>,
>;

async fn body_bytes(response: Response) -> Vec<u8> {
    to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("body should buffer")
        .to_vec()
}

/// Reusing one key with a *different* body is the conflict every client
/// integration has to handle. It must decode.
#[tokio::test]
async fn key_conflict_body_decodes_as_the_framework_error_envelope() {
    let store: Arc<dyn IdempotencyStore> = Arc::new(InMemoryIdempotencyStore::default());
    let mut svc = IdempotencyLayer::new(store, Duration::from_secs(60))
        .layer(tower::service_fn(ok_handler as fn(Request) -> OkFuture));

    let first = svc
        .call(request("/transfer", "same-key", r#"{"amount":1}"#))
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::OK);

    let conflicting = svc
        .call(request("/transfer", "same-key", r#"{"amount":999}"#))
        .await
        .unwrap();
    assert_eq!(conflicting.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let decoded: CratestackErrorResponse = CborCodec.decode(&body_bytes(conflicting).await).expect(
        "an idempotency conflict must decode as the framework error envelope — a `text/plain` \
         body is the 'unrecognized error body' bug from cratestack#846",
    );
    assert_eq!(decoded.code, "VALIDATION_ERROR");
    assert!(decoded.message.contains("idempotency_key_conflict"));
}

/// The RPC binding gets the RPC vocabulary here too, exactly as it does
/// from the rate-limit layer — transport parity, not a REST-only fix.
#[tokio::test]
async fn key_conflict_over_the_rpc_binding_uses_the_rpc_vocabulary() {
    let store: Arc<dyn IdempotencyStore> = Arc::new(InMemoryIdempotencyStore::default());
    let mut svc = IdempotencyLayer::new(store, Duration::from_secs(60))
        .layer(tower::service_fn(ok_handler as fn(Request) -> OkFuture));

    let _ = svc
        .call(request("/rpc/procedure.transfer", "k", r#"{"amount":1}"#))
        .await
        .unwrap();
    let conflicting = svc
        .call(request("/rpc/procedure.transfer", "k", r#"{"amount":9}"#))
        .await
        .unwrap();

    let decoded: RpcErrorBody = CborCodec
        .decode(&body_bytes(conflicting).await)
        .expect("RPC error envelope");
    assert_eq!(decoded.code, "invalid_argument");
}
