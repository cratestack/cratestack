//! End-to-end demo of the RPC binding's unary route. Builds the same
//! router as `main.rs`, drives it in-process via `tower::ServiceExt`, and
//! asserts the wire shape clients will see.
//!
//! Reading these tests is how you learn the example.

use cratestack::axum::body::{to_bytes, Body};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::CoolCodec;
use cratestack_codec_cbor::CborCodec;
use cratestack_codec_json::JsonCodec;
use rpc_procedures_example::build_router;
use tower::ServiceExt;

#[tokio::test]
async fn greet_procedure_round_trips_over_json() {
    let app = build_router();

    // Op id is the URL path. Body is the procedure's `Args` struct
    // directly — no envelope, no wrapper.
    let body = serde_json::json!({ "args": { "name": "world" } });
    let response = app
        .oneshot(
            Request::post("/rpc/procedure.greet")
                .header("content-type", JsonCodec::CONTENT_TYPE)
                .header("accept", JsonCodec::CONTENT_TYPE)
                .header("x-auth-id", "1")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let reply: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(reply["message"], "hello, world!");
}

#[tokio::test]
async fn greet_procedure_round_trips_over_cbor_by_default() {
    let app = build_router();

    // Same op, same body shape, different codec. Default Accept lands on
    // CBOR via `RPC_BINDING_CAPABILITIES`.
    let body = CborCodec
        .encode(&serde_json::json!({ "args": { "name": "cbor" } }))
        .unwrap();
    let response = app
        .oneshot(
            Request::post("/rpc/procedure.greet")
                .header("content-type", CborCodec::CONTENT_TYPE)
                .header("x-auth-id", "1")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let reply: serde_json::Value = CborCodec.decode(&bytes).unwrap();
    assert_eq!(reply["message"], "hello, cbor!");
}

#[tokio::test]
async fn increment_mutation_is_stateful_across_calls() {
    let app = build_router();

    // Two increments on the SAME router instance share the in-memory
    // counter. Demonstrates that handler state is preserved across calls.
    let call = |delta: i64| {
        let app = app.clone();
        async move {
            let body = serde_json::json!({ "args": { "by": delta } });
            let response = app
                .oneshot(
                    Request::post("/rpc/procedure.increment")
                        .header("content-type", JsonCodec::CONTENT_TYPE)
                        .header("accept", JsonCodec::CONTENT_TYPE)
                        .header("x-auth-id", "1")
                        .body(Body::from(serde_json::to_vec(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            let reply: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            reply["total"].as_i64().unwrap()
        }
    };

    assert_eq!(call(5).await, 5);
    assert_eq!(call(3).await, 8);
    assert_eq!(call(-1).await, 7);
}

#[tokio::test]
async fn unauthenticated_call_is_denied_with_lowercase_grpc_code() {
    let app = build_router();

    // `@allow(auth() != null)` on the procedure denies anonymous callers.
    // The RPC binding translates the underlying `CoolError::Forbidden` to
    // `RpcErrorBody { code: "permission_denied", ... }` on the wire —
    // gRPC-style lowercase, not the REST binding's SCREAMING_CASE.
    let body = serde_json::json!({ "args": { "name": "stranger" } });
    let response = app
        .oneshot(
            Request::post("/rpc/procedure.greet")
                .header("content-type", JsonCodec::CONTENT_TYPE)
                .header("accept", JsonCodec::CONTENT_TYPE)
                // intentionally no x-auth-id
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let parsed: cratestack::rpc::RpcErrorBody = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(parsed.code, "permission_denied");
}
