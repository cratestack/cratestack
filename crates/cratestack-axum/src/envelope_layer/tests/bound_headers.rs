//! Decision S1: `Idempotency-Key` and `If-Match` are bound, exactly as
//! sent, so an on-path party can neither add, strip nor alter them on a
//! signed request, and a response is bound to the ones its request carried.

use cratestack_core::request_digest;
use http::{HeaderValue, Method, StatusCode};

use super::fixtures::{Hits, REST_ROUTES, rest_router};
use super::support::*;
use crate::envelope_layer::{EnvelopeLayer, EnvelopeMode};

fn router(mode: EnvelopeMode, hits: &Hits) -> axum::Router {
    let layer = EnvelopeLayer::builder(server_envelope(), AUDIENCE, SCHEMA)
        .policy(mode)
        .rest("", &REST_ROUTES)
        .build()
        .expect("layer");
    rest_router(layer, hits)
}

const KEY: &str = "idempotency-key";

fn with_headers(
    mut req: axum::extract::Request,
    headers: &[(&'static str, &'static str)],
) -> axum::extract::Request {
    for (name, value) in headers {
        req.headers_mut()
            .append(*name, HeaderValue::from_static(value));
    }
    req
}

#[tokio::test]
async fn a_bound_header_the_client_did_not_sign_is_refused() {
    for (name, value) in [(KEY, "k1"), ("if-match", "\"3\"")] {
        let hits = Hits::default();
        let sealed = Call::new(Method::POST, "/widgets", &[]).seal(PAYLOAD).await;
        let req = with_headers(
            cose_request(Method::POST, "/widgets", sealed),
            &[(name, value)],
        );
        let answer = send(&router(EnvelopeMode::Required, &hits), req).await;
        assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{name} added");
        assert_eq!(hits.get(), 0, "{name}");
    }
}

#[tokio::test]
async fn a_signed_bound_header_stripped_or_altered_is_refused() {
    let call = || Call::new(Method::POST, "/widgets", &[]).bound(Some("k1"), Some("\"3\""));
    let cases: [(&str, &[(&'static str, &'static str)]); 4] = [
        ("key stripped", &[("if-match", "\"3\"")]),
        ("if-match stripped", &[(KEY, "k1")]),
        ("key altered", &[(KEY, "k2"), ("if-match", "\"3\"")]),
        ("if-match altered", &[(KEY, "k1"), ("if-match", "\"4\"")]),
    ];
    for &mode in &[EnvelopeMode::Required, EnvelopeMode::Optional] {
        for (what, headers) in cases {
            let hits = Hits::default();
            let sealed = call().seal(PAYLOAD).await;
            let req = with_headers(cose_request(Method::POST, "/widgets", sealed), headers);
            let answer = send(&router(mode, &hits), req).await;
            assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{what} {mode:?}");
            assert!(!answer.is_sealed(), "D4: unsigned");
            assert_eq!(hits.get(), 0, "{what}");
        }
    }
}

#[tokio::test]
async fn a_request_with_its_signed_headers_runs_and_its_response_binds_them() {
    let hits = Hits::default();
    let call = Call::new(Method::POST, "/widgets", &[]).bound(Some("k1"), Some("\"3\""));
    let sealed = call.seal(PAYLOAD).await;
    let req = with_headers(
        cose_request(Method::POST, "/widgets", sealed.clone()),
        &[(KEY, "k1"), ("if-match", "\"3\"")],
    );
    let answer = send(&router(EnvelopeMode::Required, &hits), req).await;
    assert_eq!(answer.status, StatusCode::OK);
    let digest = request_digest(&sealed);
    call.open(digest, answer.status, answer.body.clone())
        .await
        .expect("verifies with the headers bound");
    let unkeyed = Call::new(Method::POST, "/widgets", &[]);
    unkeyed
        .open(digest, answer.status, answer.body)
        .await
        .expect_err("not as the answer to the same request without them");
}

#[tokio::test]
async fn a_bound_header_sent_twice_is_refused_before_anything_is_bound() {
    let hits = Hits::default();
    let call = Call::new(Method::POST, "/widgets", &[]).bound(Some("k1"), None);
    let sealed = call.seal(PAYLOAD).await;
    let req = with_headers(
        cose_request(Method::POST, "/widgets", sealed),
        &[(KEY, "k1"), (KEY, "k1")],
    );
    let answer = send(&router(EnvelopeMode::Required, &hits), req).await;
    assert_eq!(answer.status, StatusCode::BAD_REQUEST);
    assert!(!answer.is_sealed());
    assert_eq!(hits.get(), 0);
}

/// An unsigned, nonce-bound response under `Optional` binds the headers
/// its request carried too.
#[tokio::test]
async fn a_nonce_bound_response_binds_the_idempotency_key() {
    let nonce = cratestack_core::RequestNonce::from_bytes([5; 16]);
    let req = http::Request::builder()
        .uri("/widgets/1")
        .header(cratestack_core::NONCE_HEADER, nonce.to_header_value())
        .header(http::header::ACCEPT, SIGN1)
        .header(KEY, "k1")
        .body(axum::body::Body::empty())
        .expect("request");
    let answer = send(&router(EnvelopeMode::Optional, &Hits::default()), req).await;
    assert!(answer.is_sealed());
    let digest = cratestack_core::request_digest_unsigned(&nonce, b"");
    let keyed = Call::new(Method::GET, "/widgets/{id}", &["1"]).bound(Some("k1"), None);
    keyed
        .open(digest, answer.status, answer.body.clone())
        .await
        .expect("verifies with the key");
    Call::new(Method::GET, "/widgets/{id}", &["1"])
        .open(digest, answer.status, answer.body)
        .await
        .expect_err("not without it");
}
