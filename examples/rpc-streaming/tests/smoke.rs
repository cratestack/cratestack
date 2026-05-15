//! Wire-shape demos for the same op (`procedure.ticks`) under both
//! Accept variants. The proof point is that **streaming is a content
//! negotiation**, not a route — the URL doesn't change.

use cratestack::axum::body::{to_bytes, Body};
use cratestack::axum::http::{Request, StatusCode};
use cratestack::CoolCodec;
use cratestack_codec_cbor::CborCodec;
use rpc_streaming_example::{build_router, decode_cbor_seq, schema};
use tower::ServiceExt;

#[tokio::test]
async fn ticks_returns_single_cbor_vec_with_default_accept() {
    let app = build_router();
    let body = CborCodec
        .encode(&schema::procedures::ticks::Args {
            args: schema::TickerArgs { start: 10, count: 3 },
        })
        .unwrap();
    let response = app
        .oneshot(
            Request::post("/rpc/procedure.ticks")
                .header("content-type", CborCodec::CONTENT_TYPE)
                .header("x-auth-id", "1")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();
    assert!(
        content_type.starts_with(CborCodec::CONTENT_TYPE),
        "default Accept should serve single CBOR, got `{content_type}`",
    );

    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let ticks: Vec<schema::Tick> = CborCodec.decode(&bytes).unwrap();
    assert_eq!(ticks.len(), 3);
    assert_eq!(ticks[0].value, 10);
    assert_eq!(ticks[2].value, 12);
}

#[tokio::test]
async fn ticks_streams_cbor_seq_when_negotiated() {
    let app = build_router();
    let body = CborCodec
        .encode(&schema::procedures::ticks::Args {
            args: schema::TickerArgs { start: 100, count: 4 },
        })
        .unwrap();
    let response = app
        .oneshot(
            // Same URL, same body — only the Accept header changes.
            Request::post("/rpc/procedure.ticks")
                .header("content-type", CborCodec::CONTENT_TYPE)
                .header("accept", cratestack::CBOR_SEQUENCE_CONTENT_TYPE)
                .header("x-auth-id", "1")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();
    assert!(
        content_type.starts_with(cratestack::CBOR_SEQUENCE_CONTENT_TYPE),
        "streaming Accept should advertise cbor-seq, got `{content_type}`",
    );

    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let ticks: Vec<schema::Tick> = decode_cbor_seq(&bytes);
    assert_eq!(ticks.len(), 4);
    assert_eq!(ticks[0].value, 100);
    assert_eq!(ticks[3].value, 103);
}

#[tokio::test]
async fn zero_count_returns_empty_sequence() {
    // Edge case worth pinning: empty `Sequence` ops emit zero chunks
    // and a clean end-of-body. No special marker needed.
    let app = build_router();
    let body = CborCodec
        .encode(&schema::procedures::ticks::Args {
            args: schema::TickerArgs { start: 0, count: 0 },
        })
        .unwrap();
    let response = app
        .oneshot(
            Request::post("/rpc/procedure.ticks")
                .header("content-type", CborCodec::CONTENT_TYPE)
                .header("accept", cratestack::CBOR_SEQUENCE_CONTENT_TYPE)
                .header("x-auth-id", "1")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let ticks: Vec<schema::Tick> = decode_cbor_seq(&bytes);
    assert!(ticks.is_empty());
}
