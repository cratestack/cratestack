//! Proves the `db = None` router this crate builds actually works — not
//! just that it compiles without `sqlx`. See `README.md` for the `cargo
//! tree` half of the proof (dependency-graph absence/presence).

use cratestack::CratestackCodec;
use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::futures::StreamExt;
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

/// A topic compiles and delivers under `db = None` with the `postgres`
/// feature off, where `cratestack::Topic` must be the backplane-only handle.
#[tokio::test]
async fn a_topic_delivers_with_no_database() {
    let topic = |db: no_database_verification::schema::Cratestack| db.topics().ping_echo("r1");
    let build = no_database_verification::schema::Cratestack::builder;
    let mut subscribed = topic(build().build()).subscribe().await.unwrap();

    let echo = no_database_verification::schema::PingReply {
        echo: "hi".to_owned(),
    };
    topic(build().build()).publish(&echo).await.unwrap();

    assert_eq!(subscribed.next().await.unwrap().unwrap().echo, "hi");
}
