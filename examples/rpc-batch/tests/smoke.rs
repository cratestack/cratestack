//! Wire-shape demos for `POST /rpc/batch`. Each test sends a multi-frame
//! request and asserts on the response envelope.

use cratestack::axum::body::{to_bytes, Body};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::CoolCodec;
use cratestack::rpc::{RpcRequest, RpcResponseFrame};
use cratestack_codec_cbor::CborCodec;
use rpc_batch_example::build_router;
use tower::ServiceExt;

async fn run_batch(frames: Vec<RpcRequest>) -> (StatusCode, Vec<RpcResponseFrame>) {
    let app = build_router();
    let body = CborCodec.encode(&frames).unwrap();
    let response = app
        .oneshot(
            Request::post("/rpc/batch")
                .header("content-type", CborCodec::CONTENT_TYPE)
                .header("x-auth-id", "1")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let decoded: Vec<RpcResponseFrame> = CborCodec.decode(&bytes).unwrap();
    (status, decoded)
}

#[tokio::test]
async fn batch_preserves_request_order_on_the_response() {
    // Request ids are non-monotonic on purpose — the response order
    // matches the request order, not the id order.
    let (status, responses) = run_batch(vec![
        RpcRequest {
            id: 42,
            op: "procedure.add".into(),
            input: serde_json::json!({"args": {"a": 1, "b": 2}}),
            idem: None,
        },
        RpcRequest {
            id: 7,
            op: "procedure.multiply".into(),
            input: serde_json::json!({"args": {"a": 3, "b": 4}}),
            idem: None,
        },
        RpcRequest {
            id: 99,
            op: "procedure.add".into(),
            input: serde_json::json!({"args": {"a": 10, "b": 20}}),
            idem: None,
        },
    ])
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(responses.len(), 3);

    // Order preserved.
    assert_eq!(responses[0].id, 42);
    assert_eq!(responses[1].id, 7);
    assert_eq!(responses[2].id, 99);

    // Values correct.
    assert_eq!(responses[0].output.as_ref().unwrap()["value"], 3);
    assert_eq!(responses[1].output.as_ref().unwrap()["value"], 12);
    assert_eq!(responses[2].output.as_ref().unwrap()["value"], 30);
}

#[tokio::test]
async fn per_frame_error_does_not_poison_the_batch() {
    // One bad frame (`divide` by zero) in the middle of a valid batch.
    // The envelope still returns 200; only that frame carries an error.
    let (status, responses) = run_batch(vec![
        RpcRequest {
            id: 1,
            op: "procedure.add".into(),
            input: serde_json::json!({"args": {"a": 1, "b": 1}}),
            idem: None,
        },
        RpcRequest {
            id: 2,
            op: "procedure.divide".into(),
            input: serde_json::json!({"args": {"numerator": 10, "denominator": 0}}),
            idem: None,
        },
        RpcRequest {
            id: 3,
            op: "procedure.multiply".into(),
            input: serde_json::json!({"args": {"a": 6, "b": 7}}),
            idem: None,
        },
    ])
    .await;

    assert_eq!(status, StatusCode::OK, "batch envelope must succeed");
    assert_eq!(responses.len(), 3);

    // First and third succeed.
    assert!(responses[0].error.is_none());
    assert_eq!(responses[0].output.as_ref().unwrap()["value"], 2);
    assert!(responses[2].error.is_none());
    assert_eq!(responses[2].output.as_ref().unwrap()["value"], 42);

    // Middle frame errors with the gRPC-style code from the RpcErrorBody
    // post-processor.
    let err = responses[1].error.as_ref().expect("frame 2 should error");
    assert_eq!(err.code, "failed_precondition");
    assert!(err.message.contains("denominator"), "msg: {}", err.message);
}

#[tokio::test]
async fn unknown_op_in_batch_returns_per_frame_not_found() {
    // Mixing a valid op with an unknown one. The envelope succeeds; the
    // bad frame carries a `not_found` error.
    let (status, responses) = run_batch(vec![
        RpcRequest {
            id: 1,
            op: "procedure.add".into(),
            input: serde_json::json!({"args": {"a": 1, "b": 1}}),
            idem: None,
        },
        RpcRequest {
            id: 2,
            op: "procedure.does_not_exist".into(),
            input: serde_json::json!(null),
            idem: None,
        },
    ])
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(responses.len(), 2);
    assert!(responses[0].error.is_none());
    let err = responses[1].error.as_ref().unwrap();
    assert_eq!(err.code, "not_found");
}

#[tokio::test]
async fn idempotency_key_header_is_rejected_on_batch() {
    // Per-frame `idem` is the only model on the batch route; the HTTP
    // header is ambiguous and explicitly rejected.
    let app = build_router();
    let body = CborCodec.encode(&Vec::<RpcRequest>::new()).unwrap();
    let response = app
        .oneshot(
            Request::post("/rpc/batch")
                .header("content-type", CborCodec::CONTENT_TYPE)
                .header("x-auth-id", "1")
                .header("idempotency-key", "client-key-abc")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
