//! GHSA-r67q-4qqq-g9gm: an `@isolation` procedure out of retries committed
//! nothing, so the layer must not record its `TRANSACTION_ABORTED` response —
//! the same `Idempotency-Key` runs the handler again. Every other error is
//! still recorded and replayed, including a `TRANSACTION_ABORTED` the
//! answering dispatch did not claim as its own (a caller that propagated
//! another procedure's abort may have committed work), which is answered as
//! a 500 `INTERNAL_ERROR`. Drives the real
//! `Service` with the error encoded the way generated handlers encode it,
//! on REST and through RPC's error re-encoding.

#![cfg(test)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use axum::body::Body;
use axum::extract::Request;
use axum::response::Response;
use cratestack_codec_cbor::CborCodec;
use cratestack_core::{AbortOwnership, CratestackError, DbErrorInfo, TransactionAbort};
use http::{HeaderMap, StatusCode};
use tower::{Layer, Service};

use super::layer::IdempotencyLayer;
use super::store::IdempotencyStore;
use super::tests_stream_bypass::InMemoryIdempotencyStore;

fn request() -> Request {
    Request::builder()
        .method("POST")
        .uri("/$procs/withdraw")
        .header("idempotency-key", "k-1")
        .header("authorization", "Bearer test")
        .body(Body::from("same body"))
        .unwrap()
}

/// Each state built the only way code outside `cratestack-core` can:
/// `Claimed` is the generated dispatch's claim of an exhausted abort.
fn aborted_with(ownership: AbortOwnership) -> CratestackError {
    let info = DbErrorInfo {
        detail: "retry the request".to_owned(),
        sqlstate: Some("40001".to_owned()),
        constraint: None,
    };
    let error = match ownership {
        AbortOwnership::Propagated => {
            CratestackError::TransactionAborted(TransactionAbort::propagated(info))
        }
        _ => CratestackError::TransactionAborted(TransactionAbort::exhausted(info)),
    };
    if ownership == AbortOwnership::Claimed {
        error.__generated_claim_transaction_abort()
    } else {
        error
    }
}

/// What an `@isolation` procedure's own dispatch answers when its retries
/// ran out.
fn aborted() -> CratestackError {
    aborted_with(AbortOwnership::Claimed)
}

/// Calls the layer twice with one key; returns both statuses and how many
/// times the handler ran.
async fn twice<F, Fut>(respond: F) -> (StatusCode, StatusCode, usize)
where
    F: Fn() -> Fut + Clone + Send + 'static,
    Fut: std::future::Future<Output = Response> + Send,
{
    let store: Arc<dyn IdempotencyStore> = Arc::new(InMemoryIdempotencyStore::default());
    let runs = Arc::new(AtomicUsize::new(0));
    let counter = runs.clone();
    let inner = tower::service_fn(move |_req: Request| {
        let (counter, respond) = (counter.clone(), respond.clone());
        async move {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok::<_, std::convert::Infallible>(respond().await)
        }
    });
    let mut svc = IdempotencyLayer::new(store, Duration::from_secs(60)).layer(inner);
    let first = svc.call(request()).await.unwrap().status();
    let second = svc.call(request()).await.unwrap().status();
    (first, second, runs.load(Ordering::SeqCst))
}

#[tokio::test]
async fn an_aborted_rest_response_is_not_recorded() {
    let (first, second, runs) =
        twice(|| async { crate::codec::encode_codec_result::<_, ()>(&CborCodec, Err(aborted())) })
            .await;
    assert_eq!(
        (first, second),
        (StatusCode::CONFLICT, StatusCode::CONFLICT)
    );
    assert_eq!(runs, 2, "the same key must run the handler again");
}

#[tokio::test]
async fn an_aborted_rpc_response_is_not_recorded() {
    let (_, _, runs) = twice(|| async {
        let handler = crate::codec::encode_codec_result::<_, ()>(&CborCodec, Err(aborted()));
        crate::rpc::convert_handler_error_response(handler, &CborCodec, &HeaderMap::new()).await
    })
    .await;
    assert_eq!(runs, 2, "RPC's re-encoded error must keep the tag");

    let (_, _, runs) =
        twice(|| async { crate::rpc::encode_rpc_error(&CborCodec, &HeaderMap::new(), &aborted()) })
            .await;
    assert_eq!(runs, 2, "a dispatcher-raised aborted error is not recorded");
}

#[tokio::test]
async fn any_other_error_is_still_recorded() {
    let (first, second, runs) = twice(|| async {
        crate::codec::encode_codec_result::<_, ()>(
            &CborCodec,
            Err(CratestackError::Conflict("duplicate".to_owned())),
        )
    })
    .await;
    assert_eq!(
        (first, second),
        (StatusCode::CONFLICT, StatusCode::CONFLICT)
    );
    assert_eq!(runs, 1, "a CONFLICT is recorded and replayed, as before");
}

/// An abort the answering dispatch did not claim — propagated out of a
/// body, or never claimed — is answered as `500 INTERNAL_ERROR`, not as
/// "nothing committed, send it again", and recorded like any other error:
/// through the codec and transport encoders, a dispatcher-raised RPC error,
/// and RPC's re-encoding.
#[tokio::test]
async fn an_unclaimed_abort_is_an_internal_error_and_recorded() {
    type Encode = fn(AbortOwnership) -> Response;
    let encoders: [(&str, Encode); 4] = [
        ("codec", |ownership| {
            crate::codec::encode_codec_result::<_, ()>(&CborCodec, Err(aborted_with(ownership)))
        }),
        ("transport", |ownership| {
            crate::encode_transport_result_with_status_for::<_, ()>(
                &CborCodec,
                &HeaderMap::new(),
                &crate::rpc::RPC_BINDING_CAPABILITIES,
                StatusCode::OK,
                Err(aborted_with(ownership)),
            )
        }),
        ("rpc dispatcher", |ownership| {
            crate::rpc::encode_rpc_error(&CborCodec, &HeaderMap::new(), &aborted_with(ownership))
        }),
        ("rest encoded", |ownership| {
            crate::codec::encode_codec_result::<_, ()>(&CborCodec, Err(aborted_with(ownership)))
        }),
    ];
    for ownership in [AbortOwnership::Propagated, AbortOwnership::Exhausted] {
        for (label, encode) in encoders {
            let (first, second, runs) = twice(move || async move {
                let response = encode(ownership);
                if label == "rest encoded" {
                    let headers = HeaderMap::new();
                    crate::rpc::convert_handler_error_response(response, &CborCodec, &headers).await
                } else {
                    response
                }
            })
            .await;
            assert_eq!(
                (first, second),
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    StatusCode::INTERNAL_SERVER_ERROR
                ),
                "{label} {ownership:?}"
            );
            assert_eq!(runs, 1, "{label} {ownership:?} is recorded and replayed");
        }
    }
}
