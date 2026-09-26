//! `Off`: plain traffic is untouched, and a COSE body is refused rather
//! than forwarded unverified.

use http::{Method, StatusCode, header};

use super::fixtures::{Hits, REST_ROUTES, rest_router};
use super::support::*;
use crate::envelope_layer::{EnvelopeLayer, EnvelopeMode};

fn router(hits: &Hits) -> axum::Router {
    let layer = EnvelopeLayer::builder(server_envelope(), AUDIENCE, SCHEMA)
        .policy(EnvelopeMode::Off)
        .rest("", &REST_ROUTES)
        .build()
        .expect("layer");
    rest_router(layer, hits)
}

#[tokio::test]
async fn plain_traffic_is_untouched() {
    let hits = Hits::default();
    let mut req = plain_request(Method::POST, "/widgets", PAYLOAD);
    req.headers_mut().insert(
        header::ACCEPT,
        http::HeaderValue::from_static("application/cbor, */*"),
    );
    let answer = send(&router(&hits), req).await;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(answer.seen("x-seen-accept"), "application/cbor, */*");
    assert_eq!(answer.seen("x-seen-principal"), "<absent>");
    assert!(!answer.is_sealed());
}

#[tokio::test]
async fn a_cose_body_is_refused_not_forwarded() {
    let hits = Hits::default();
    let sealed = Call::new(Method::POST, "/widgets", &[]).seal(PAYLOAD).await;
    let answer = send(
        &router(&hits),
        cose_request(Method::POST, "/widgets", sealed),
    )
    .await;
    assert_eq!(answer.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert!(!answer.is_sealed());
    assert_eq!(hits.get(), 0, "nothing behind the layer saw the COSE bytes");
}
