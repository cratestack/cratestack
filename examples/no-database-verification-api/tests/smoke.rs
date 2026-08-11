//! Proves the `db = None` router this crate builds actually works under
//! `cratestack-api` — not just that it compiles without `sqlx`. See
//! `README.md` for the `cargo tree` half of the proof (dependency-graph
//! absence).

use cratestack::CoolCodec;
use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack_codec_json::JsonCodec;
use no_database_verification_api::build_router;
use tower::ServiceExt;

#[tokio::test]
async fn ping_procedure_round_trips_over_http_with_no_database() {
    let app = build_router();

    let body = serde_json::json!({ "args": { "message": "hello" } });
    let response = app
        .oneshot(
            Request::post("/$procs/ping")
                .header("content-type", JsonCodec::CONTENT_TYPE)
                .header("accept", JsonCodec::CONTENT_TYPE)
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let reply: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(reply["echo"], "hello");
}

/// cratestack#407: `submit` declares `@status(202)` — the generated axum
/// handler must emit `202 Accepted` on `Ok(...)`, not the hardcoded `200`
/// every procedure got before this feature existed.
#[tokio::test]
async fn submit_procedure_returns_the_declared_202_status() {
    let app = build_router();

    let body = serde_json::json!({ "args": { "message": "hello" } });
    let response = app
        .oneshot(
            Request::post("/$procs/submit")
                .header("content-type", JsonCodec::CONTENT_TYPE)
                .header("accept", JsonCodec::CONTENT_TYPE)
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let reply: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(reply["echo"], "hello");
}

/// cratestack#407 follow-up: `streamed` is `@stream`-marked AND declares
/// `@status(202)`. `procedure_dispatch_tail_tokens`'s `@stream` branch used
/// to discard the declared status and hardcode `StatusCode::OK` instead of
/// threading it through `encode_transport_stream_result_with_status_for`
/// the way the unary/list branches already did — so this combination
/// silently no-opped (always `200`, never erroring) instead of either
/// working or being rejected at schema-compile time. This is a real
/// `POST /$procs/streamed` round trip against a procedure implemented as
/// a genuine `impl Stream` (`src/lib.rs`'s `Procedures::streamed`), with a
/// plain JSON `Accept` (the buffered fallback inside the stream branch,
/// per `encode_transport_stream_result_with_status_for`'s doc comment —
/// still the same call site and `success_status` argument this ticket
/// fixed, just not the incremental `cbor-seq` sub-path).
#[tokio::test]
async fn streamed_procedure_returns_the_declared_202_status() {
    let app = build_router();

    let body = serde_json::json!({ "args": { "message": "hello" } });
    let response = app
        .oneshot(
            Request::post("/$procs/streamed")
                .header("content-type", JsonCodec::CONTENT_TYPE)
                .header("accept", JsonCodec::CONTENT_TYPE)
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let reply: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(reply[0]["echo"], "hello");
}
