//! Proves the `db = None` router this crate builds actually works — not
//! just that it compiles without `sqlx`. See `README.md` for the `cargo
//! tree` half of the proof (dependency-graph absence/presence).

use cratestack::CoolCodec;
use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack_codec_json::JsonCodec;
use no_database_verification::build_router;
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
