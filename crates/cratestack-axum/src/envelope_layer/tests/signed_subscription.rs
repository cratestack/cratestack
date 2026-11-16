//! A signed open of a subscription marked `@frames(unsigned)` streams plain
//! frames (cratestack#1378): `Cratestack-Frames: unsigned`, never sealed, the
//! handler seeing the verified principal, a stream `Accept` and a deadline.

use axum::routing::get;
use http::{Method, StatusCode};

use super::fixtures::Hits;
use super::signed_subscription_support::*;
use super::support::*;

const BOTH: &str = "application/cbor-seq, application/cose; cose-type=\"cose-sign1\"";

#[tokio::test]
async fn a_signed_get_or_post_open_streams_unsigned_frames_in_the_asked_type() {
    for method in [Method::GET, Method::POST] {
        for (accept, stream_type) in [
            (BOTH, CBOR_SEQ),
            (
                "text/event-stream, application/cose; cose-type=\"cose-sign1\"",
                SSE,
            ),
        ] {
            let hits = Hits::default();
            let router = router(builder().build().expect("layer"), &hits, Serve::Items);
            let (request, _call, _sealed) = open(method.clone(), MARKED, &[accept]).await;
            let answer = send(&router, request).await;
            let at = format!("{method} {stream_type}");
            assert_eq!(answer.status, StatusCode::OK, "{at}");
            assert_eq!(answer.content_type(), stream_type, "{at}");
            assert_eq!(answer.seen("cratestack-frames"), "unsigned", "{at}");
            assert_eq!(answer.seen("cache-control"), "no-store", "{at}");
            assert_eq!(answer.seen("cratestack-payload-type"), "<absent>", "{at}");
            assert_eq!(answer.seen("x-seen-accept"), stream_type, "{at}");
            assert_eq!(answer.seen("x-seen-principal"), CLIENT_PRINCIPAL, "{at}");
            assert_ne!(answer.seen("x-seen-deadline-ms"), "<none>", "{at}");
            assert!(!answer.is_sealed(), "frames are plain: {at}");
            assert!(!answer.body.is_empty(), "{at}");
            assert_eq!(hits.get(), 1, "{at}");
        }
    }
}

#[tokio::test]
async fn the_first_literal_stream_type_wins() {
    let hits = Hits::default();
    let router = router(builder().build().expect("layer"), &hits, Serve::Items);
    let both = "text/event-stream, application/cbor-seq, */*";
    let (request, ..) = open(Method::GET, MARKED, &[both]).await;
    let answer = send(&router, request).await;
    assert_eq!(answer.content_type(), SSE);
    assert_eq!(answer.seen("x-seen-accept"), SSE);
}

#[tokio::test]
async fn an_unsigned_open_under_required_is_the_coarse_401() {
    let hits = Hits::default();
    let router = router(builder().build().expect("layer"), &hits, Serve::Items);
    for method in [Method::GET, Method::POST] {
        let mut request = plain_request(method.clone(), MARKED_PATH, b"");
        request.headers_mut().insert(
            http::header::ACCEPT,
            http::HeaderValue::from_static(CBOR_SEQ),
        );
        let answer = send(&router, request).await;
        assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{method}");
    }
    assert_eq!(hits.get(), 0);
}

#[tokio::test]
async fn a_put_to_the_subscribe_route_is_a_405() {
    let hits = Hits::default();
    let router = router(builder().build().expect("layer"), &hits, Serve::Items);
    let answer = send(&router, plain_request(Method::PUT, MARKED_PATH, b"")).await;
    assert_eq!(answer.status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(hits.get(), 0);
}

/// A refusal the subscribe handler writes (`403` here) is an error, not a
/// stream: it goes back sealed, never plain.
#[tokio::test]
async fn a_non_2xx_answer_is_sealed_not_plain() {
    let hits = Hits::default();
    let router = router(builder().build().expect("layer"), &hits, Serve::Forbidden);
    let (request, call, sealed) = open(Method::GET, MARKED, &[BOTH]).await;
    let answer = send(&router, request).await;
    assert_eq!(answer.status, StatusCode::FORBIDDEN);
    assert!(answer.is_sealed(), "{}", answer.content_type());
    assert_eq!(answer.seen("cratestack-frames"), "<absent>");
    call.open(
        cratestack_core::request_digest(&sealed),
        answer.status,
        answer.body,
    )
    .await
    .expect("sealed under the open's binding");
}

/// The stream the handler produced is not the type the open asked for: not
/// an unsigned stream the client expects, so it is replaced.
#[tokio::test]
async fn a_stream_in_another_type_than_asked_is_replaced_by_a_sealed_406() {
    let layer = builder().build().expect("layer");
    let lying = || async {
        let mut response = axum::response::Response::new(axum::body::Body::from("x"));
        response.headers_mut().insert(
            http::header::CONTENT_TYPE,
            http::HeaderValue::from_static(CBOR_SEQ),
        );
        response
            .extensions_mut()
            .insert(crate::transport::StreamedResponseMarker);
        response
    };
    let router = axum::Router::new()
        .route("/rpc/subscribe/{op_id}", get(lying))
        .layer(layer);
    let (request, ..) = open(Method::GET, MARKED, &["text/event-stream"]).await;
    let answer = send(&router, request).await;
    assert_eq!(answer.status, StatusCode::NOT_ACCEPTABLE);
    assert!(answer.is_sealed());
}

/// A 200 in the asked type that no subscription binding produced (no
/// `StreamedResponseMarker`) is not a stream: it is sealed, never passed plain.
#[tokio::test]
async fn a_body_in_the_asked_type_that_is_no_stream_is_sealed_not_passed_plain() {
    let layer = builder().build().expect("layer");
    let imitation = || async {
        let mut response = axum::response::Response::new(axum::body::Body::from("x"));
        response.headers_mut().insert(
            http::header::CONTENT_TYPE,
            http::HeaderValue::from_static(CBOR_SEQ),
        );
        response
    };
    let router = axum::Router::new()
        .route("/rpc/subscribe/{op_id}", get(imitation))
        .layer(layer);
    let (request, ..) = open(Method::GET, MARKED, &[CBOR_SEQ]).await;
    let answer = send(&router, request).await;
    assert!(answer.is_sealed(), "{}", answer.content_type());
    assert_eq!(answer.seen("cratestack-frames"), "<absent>");
}
