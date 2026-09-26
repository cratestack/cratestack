//! Hostile or buggy plug-ins cannot break the layer's invariants: a COSE
//! body is always opened or refused, a resolver and a policy are asked once,
//! a principal mapper only ever sees a verified request, and a seal policy
//! is asked only about what it may decide.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::body::Body;
use http::{Method, StatusCode, header};

use super::fixtures::{Hits, REST_ROUTES, rest_router};
use super::support::*;
use super::toy::{Toy, toy_request};
use crate::envelope_layer::{
    BindingResolver, EnvelopeLayer, EnvelopeMode, PolicyRequest, Resolution, ResolvedRoute,
    RouteRequest, UnsignedRequest, VerifiedRequest,
};

fn toy_layer(toy: Toy, mode: EnvelopeMode) -> EnvelopeLayer {
    EnvelopeLayer::builder(toy, AUDIENCE, SCHEMA)
        .policy(mode)
        .rest("", &REST_ROUTES)
        .build()
        .expect("layer")
}

#[tokio::test]
async fn a_policy_saying_off_cannot_let_a_cose_body_through() {
    let (hits, toy) = (Hits::default(), Toy::default());
    let layer = EnvelopeLayer::builder(toy.clone(), AUDIENCE, SCHEMA)
        .policy(|_: &PolicyRequest<'_>| EnvelopeMode::Off)
        .rest("", &REST_ROUTES)
        .build()
        .expect("layer");
    let req = toy_request(Method::POST, "/widgets", "application/cose", b"TOY:x");
    let answer = send(&rest_router(layer, &hits), req).await;
    assert_eq!(answer.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!((hits.get(), toy.opens()), (0, 0));
}

#[tokio::test]
async fn cose_is_recognised_by_the_layer_not_by_the_envelope() {
    // `Toy::default()` claims no content type at all.
    for content_type in [
        "APPLICATION/COSE",
        "application/cose ; cose-type=\"cose-sign1\"",
    ] {
        let (hits, toy) = (Hits::default(), Toy::default());
        let router = rest_router(toy_layer(toy.clone(), EnvelopeMode::Optional), &hits);
        let answer = send(
            &router,
            toy_request(Method::POST, "/widgets", content_type, b"TOY:x"),
        )
        .await;
        assert_eq!(answer.status, StatusCode::OK, "{content_type}");
        assert_eq!(toy.opens(), 1, "opened: {content_type}");
        assert_eq!(answer.seen("x-seen-content-type"), "application/cbor");
    }
}

#[tokio::test]
async fn a_second_content_type_header_does_not_smuggle_cose_past_the_opener() {
    let (hits, toy) = (Hits::default(), Toy::default());
    let router = rest_router(toy_layer(toy.clone(), EnvelopeMode::Optional), &hits);
    let req = http::Request::builder()
        .method(Method::POST)
        .uri("/widgets")
        .header(header::CONTENT_TYPE, "application/cbor")
        .header(header::CONTENT_TYPE, "application/cose")
        .body(Body::from("not a toy message"))
        .expect("request");
    let answer = send(&router, req).await;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED);
    assert_eq!((toy.opens(), hits.get()), (1, 0));
}

/// Answers a different route every time it is asked.
struct Fickle(AtomicUsize);

impl BindingResolver for Fickle {
    fn resolve(&self, request: &RouteRequest<'_>) -> Resolution {
        if request.matched_path().is_none() {
            return Resolution::Unresolved;
        }
        let call = self.0.fetch_add(1, Ordering::SeqCst);
        Resolution::Op(ResolvedRoute::new(
            format!("route-{call}"),
            vec![call.to_string()],
        ))
    }
}

#[tokio::test]
async fn a_fickle_resolver_cannot_split_the_request_and_response_bindings() {
    let (hits, toy) = (Hits::default(), Toy::default());
    let layer = EnvelopeLayer::builder(toy.clone(), AUDIENCE, SCHEMA)
        .policy(EnvelopeMode::Required)
        .binding_resolver(Fickle(AtomicUsize::new(0)))
        .build()
        .expect("layer");
    let req = toy_request(Method::POST, "/widgets", "application/cose", b"TOY:x");
    let answer = send(&rest_router(layer, &hits), req).await;
    let opened = toy.opened.lock().expect("lock").clone();
    assert_eq!(opened, vec![("route-0".to_owned(), vec!["0".to_owned()])]);
    assert_eq!(
        answer.body.as_ref(),
        b"TOYRESP:route-0:200:x",
        "sealed for the same route"
    );
}

#[tokio::test]
async fn the_principal_mapper_never_runs_for_an_unverified_request() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let mapper = move |_: &VerifiedRequest<'_>| {
        counted.fetch_add(1, Ordering::SeqCst);
        Ok("p".to_owned())
    };
    let (hits, toy) = (Hits::default(), Toy::default());
    let layer = EnvelopeLayer::builder(toy, AUDIENCE, SCHEMA)
        .policy(EnvelopeMode::Optional)
        .rest("", &REST_ROUTES)
        .principal_mapper(mapper)
        .build()
        .expect("layer");
    let router = rest_router(layer, &hits);
    let bad = send(
        &router,
        toy_request(Method::POST, "/widgets", "application/cose", b"NOPE"),
    )
    .await;
    assert_eq!(bad.status, StatusCode::UNAUTHORIZED);
    let plain = send(&router, plain_request(Method::POST, "/widgets", PAYLOAD)).await;
    assert_eq!(plain.status, StatusCode::OK);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn an_empty_principal_is_refused_with_a_sealed_500() {
    let (hits, toy) = (Hits::default(), Toy::default());
    let layer = EnvelopeLayer::builder(toy, AUDIENCE, SCHEMA)
        .policy(EnvelopeMode::Required)
        .rest("", &REST_ROUTES)
        .principal_mapper(|_: &VerifiedRequest<'_>| Ok(String::new()))
        .build()
        .expect("layer");
    let req = toy_request(Method::POST, "/widgets", "application/cose", b"TOY:x");
    let answer = send(&rest_router(layer, &hits), req).await;
    assert_eq!(answer.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(answer.body.starts_with(b"TOYRESP:/widgets:500:"), "sealed");
    assert_eq!(hits.get(), 0);
}

#[tokio::test]
async fn the_seal_policy_is_asked_only_about_nonce_bound_unsigned_requests() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let always = move |_: &UnsignedRequest<'_>| {
        counted.fetch_add(1, Ordering::SeqCst);
        true
    };
    let hits = Hits::default();
    let layer = EnvelopeLayer::builder(server_envelope(), AUDIENCE, SCHEMA)
        .policy(EnvelopeMode::Optional)
        .rest("", &REST_ROUTES)
        .response_seal_policy(always)
        .build()
        .expect("layer");
    let router = rest_router(layer, &hits);
    let no_nonce = send(&router, plain_request(Method::GET, "/widgets/1", b"")).await;
    assert!(
        !no_nonce.is_sealed(),
        "no nonce: nothing to bind the answer to"
    );
    let sealed = Call::new(Method::POST, "/widgets", &[]).seal(PAYLOAD).await;
    let signed = send(&router, cose_request(Method::POST, "/widgets", sealed)).await;
    assert!(
        signed.is_sealed(),
        "a signed request's answer is sealed regardless"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
