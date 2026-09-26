//! `Required` over REST: everything is signed (D3), every response of a
//! generated op is sealed, and the layer's own refusals are not (D4).

use cratestack_cose::request_digest;
use http::{Method, StatusCode};

use super::fixtures::{Hits, REST_ROUTES, rest_router};
use super::support::*;
use crate::envelope_layer::{EnvelopeLayer, EnvelopeMode};

fn router(hits: &Hits) -> axum::Router {
    let layer = EnvelopeLayer::builder(server_envelope(), AUDIENCE, SCHEMA)
        .policy(EnvelopeMode::Required)
        .rest("", &REST_ROUTES)
        .build()
        .expect("layer");
    rest_router(layer, hits)
}

#[tokio::test]
async fn a_signed_post_reaches_the_router_as_plain_cbor_and_its_response_verifies() {
    let hits = Hits::default();
    let call = Call::new(Method::POST, "/widgets", &[]);
    let sealed = call.seal(PAYLOAD).await;
    let answer = send(
        &router(&hits),
        cose_request(Method::POST, "/widgets", sealed.clone()),
    )
    .await;

    assert_eq!(answer.status, StatusCode::OK);
    assert!(answer.is_sealed(), "{}", answer.content_type());
    assert_eq!(answer.seen("x-seen-content-type"), "application/cbor");
    assert_eq!(answer.seen("x-seen-accept"), "application/cbor");
    assert_eq!(answer.seen("x-seen-principal"), CLIENT_PRINCIPAL);
    assert_eq!(answer.seen("x-seen-signer-alg"), "-19");
    let payload = call
        .open(request_digest(&sealed), answer.status, answer.body)
        .await
        .expect("the response verifies against the request");
    assert_eq!(
        payload.as_ref(),
        PAYLOAD,
        "the router saw the opened payload"
    );
}

#[tokio::test]
async fn bodiless_methods_seal_an_empty_payload_and_arrive_bodiless() {
    for method in [Method::GET, Method::DELETE] {
        let hits = Hits::default();
        let call = Call::new(method.clone(), "/widgets/{id}", &["1"]);
        let sealed = call.seal(&[]).await;
        let answer = send(
            &router(&hits),
            cose_request(method, "/widgets/1", sealed.clone()),
        )
        .await;
        assert_eq!(answer.status, StatusCode::OK);
        assert_eq!(answer.seen("x-seen-content-type"), "<absent>");
        call.open(request_digest(&sealed), answer.status, answer.body)
            .await
            .expect("verifies");
    }
}

#[tokio::test]
async fn an_unsigned_request_is_refused_unsigned_before_the_router_runs() {
    let hits = Hits::default();
    let router = router(&hits);
    for req in [
        plain_request(Method::POST, "/widgets", PAYLOAD),
        plain_request(Method::GET, "/widgets/1", b""),
    ] {
        let answer = send(&router, req).await;
        assert_eq!(answer.status, StatusCode::UNAUTHORIZED);
        assert_eq!(answer.content_type(), "application/cbor", "D4: unsigned");
        assert_eq!(error_code(&answer.body), "UNAUTHORIZED");
    }
    assert_eq!(hits.get(), 0);
}

#[tokio::test]
async fn a_tampered_or_stripped_request_is_the_same_coarse_401() {
    let hits = Hits::default();
    let router = router(&hits);
    let call = Call::new(Method::POST, "/widgets", &[]);
    let mut tampered = call.seal(PAYLOAD).await.to_vec();
    let last = tampered.len() - 1;
    tampered[last] ^= 1;
    let unsigned = send(&router, plain_request(Method::POST, "/widgets", PAYLOAD)).await;
    for body in [tampered, PAYLOAD.to_vec()] {
        let answer = send(&router, cose_request(Method::POST, "/widgets", body.into())).await;
        assert_eq!(answer.status, StatusCode::UNAUTHORIZED);
        assert!(!answer.is_sealed());
        assert_eq!(
            answer.body, unsigned.body,
            "one coarse 401, whatever failed"
        );
    }
    assert_eq!(hits.get(), 0);
}

#[tokio::test]
async fn a_replayed_request_is_refused() {
    let hits = Hits::default();
    let router = router(&hits);
    let sealed = Call::new(Method::POST, "/widgets", &[]).seal(PAYLOAD).await;
    let first = send(
        &router,
        cose_request(Method::POST, "/widgets", sealed.clone()),
    )
    .await;
    assert_eq!(first.status, StatusCode::OK);
    let replay = send(&router, cose_request(Method::POST, "/widgets", sealed)).await;
    assert_eq!(replay.status, StatusCode::UNAUTHORIZED);
    assert!(
        !replay.is_sealed(),
        "D4: a replay never earns a signed answer"
    );
    assert_eq!(hits.get(), 1);
}

#[tokio::test]
async fn path_parameters_are_bound_both_ways() {
    let hits = Hits::default();
    let router = router(&hits);
    let one = Call::new(Method::GET, "/widgets/{id}", &["1"]);
    let two = Call::new(Method::GET, "/widgets/{id}", &["2"]);

    let swapped = send(
        &router,
        cose_request(Method::GET, "/widgets/2", one.seal(&[]).await),
    )
    .await;
    assert_eq!(
        swapped.status,
        StatusCode::UNAUTHORIZED,
        "sealed for /widgets/1"
    );

    let sealed = one.seal(&[]).await;
    let answer = send(
        &router,
        cose_request(Method::GET, "/widgets/1", sealed.clone()),
    )
    .await;
    let digest = request_digest(&sealed);
    two.open(digest, answer.status, answer.body.clone())
        .await
        .expect_err("the answer for /widgets/1 is not the answer for /widgets/2");
    one.open(digest, answer.status, answer.body)
        .await
        .expect("verifies");
}

#[tokio::test]
async fn the_canonical_query_is_bound() {
    let hits = Hits::default();
    let router = router(&hits);
    let mut call = Call::new(Method::GET, "/widgets/{id}", &["1"]);
    call.query = Some("a=1&b=2");
    let sealed = call.seal(&[]).await;
    let answer = send(
        &router,
        cose_request(Method::GET, "/widgets/1?b=2&a=1", sealed.clone()),
    )
    .await;
    assert_eq!(
        answer.status,
        StatusCode::OK,
        "spelling differences canonicalise"
    );
    call.open(request_digest(&sealed), answer.status, answer.body)
        .await
        .expect("verifies");

    let other = send(
        &router,
        cose_request(Method::GET, "/widgets/1?a=1&b=3", call.seal(&[]).await),
    )
    .await;
    assert_eq!(other.status, StatusCode::UNAUTHORIZED);
}
