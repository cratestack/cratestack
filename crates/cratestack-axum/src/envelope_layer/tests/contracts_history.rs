//! Binding version 2, several accepted digests for one op (cratestack#1123):
//! what a compatible-contract lock adds to the table later. The response is
//! sealed under the digest the request opened under, the accepted digests
//! are tried newest first when the client names none, and the trial cap
//! holds. Single-digest behaviour is `contracts.rs`.

use std::sync::atomic::Ordering;

use axum::body::Body;
use cratestack_core::{
    AcceptedContracts, NONCE_HEADER, RequestNonce, request_digest, request_digest_unsigned,
};
use http::{Method, StatusCode, header};

use super::contracts_support::{
    NEW, OLD, WITH_HISTORY, post_widgets, rest, rest_counting, selecting,
};
use super::fixtures::{Hits, REST_ROUTES, rest_router};
use super::support::*;
use crate::envelope_layer::{DEFAULT_MAX_CONTRACT_TRIALS, EnvelopeLayer, EnvelopeMode};

#[tokio::test]
async fn an_older_accepted_digest_opens_and_the_response_is_sealed_under_it_not_the_current() {
    let hits = Hits::default();
    let (call, sealed, req) = post_widgets(OLD, Some(OLD)).await;
    let answer = send(&rest(WITH_HISTORY, None, &hits), req).await;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(hits.get(), 1);
    let body = answer.body.clone();
    call.open(request_digest(&sealed), answer.status, answer.body)
        .await
        .expect("sealed under OLD, the digest the request bound");
    let under_current = call.clone_with_contract(NEW);
    under_current
        .open(request_digest(&sealed), StatusCode::OK, body)
        .await
        .expect_err("not sealed under the server's current digest");
}

#[tokio::test]
async fn without_a_selector_the_accepted_digests_are_tried_newest_first() {
    let hits = Hits::default();
    let (call, sealed, req) = post_widgets(OLD, None).await;
    let (router, opens) = rest_counting(WITH_HISTORY, None, &hits);
    let answer = send(&router, req).await;
    assert_eq!(answer.status, StatusCode::OK, "NEW fails, OLD verifies");
    assert_eq!(opens.load(Ordering::SeqCst), 2, "NEW, then OLD");
    call.open(request_digest(&sealed), answer.status, answer.body)
        .await
        .expect("sealed under OLD");
    // The failed NEW trial burned nothing, and the success recorded the
    // nonce: the very same message is accepted exactly once.
    let replay = send(
        &router,
        cose_request(Method::POST, "/widgets", sealed.clone()),
    )
    .await;
    assert_eq!(replay.status, StatusCode::UNAUTHORIZED);
    assert_eq!(hits.get(), 1);
}

#[tokio::test]
async fn the_trial_cap_is_honoured() {
    let hits = Hits::default();
    let (_, _, req) = post_widgets(OLD, None).await;
    let (router, opens) = rest_counting(WITH_HISTORY, Some(1), &hits);
    let answer = send(&router, req).await;
    assert_eq!(
        answer.status,
        StatusCode::UNAUTHORIZED,
        "only NEW was tried; OLD is beyond the cap"
    );
    assert_eq!(hits.get(), 0);
    assert_eq!(opens.load(Ordering::SeqCst), 1, "one trial, not two");
}

#[tokio::test]
async fn a_forged_request_without_the_header_costs_the_default_cap_in_opens() {
    static FIVE: AcceptedContracts = &[(
        "POST /widgets",
        &[NEW, OLD, [0x23; 32], [0x24; 32], [0x25; 32]],
    )];
    let hits = Hits::default();
    let (_, _, req) = post_widgets([0x44; 32], None).await;
    let (router, opens) = rest_counting(FIVE, None, &hits);
    let answer = send(&router, req).await;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED);
    assert_eq!(hits.get(), 0);
    assert_eq!(
        opens.load(Ordering::SeqCst),
        DEFAULT_MAX_CONTRACT_TRIALS,
        "five accepted digests, four opens"
    );
    assert_eq!(DEFAULT_MAX_CONTRACT_TRIALS, 4);
}

#[tokio::test]
async fn an_unsigned_request_is_answered_under_the_digest_its_selector_names() {
    static GETS: AcceptedContracts = &[("GET /widgets/{id}", &[NEW, OLD])];
    let hits = Hits::default();
    let layer = EnvelopeLayer::builder(server_envelope(), AUDIENCE, GETS)
        .policy(EnvelopeMode::Optional)
        .rest("", &REST_ROUTES)
        .build()
        .expect("layer");
    let router = rest_router(layer, &hits);
    let nonce = RequestNonce::from_bytes([9; 16]);
    let get = |selector: Option<[u8; 32]>| {
        let req = http::Request::builder()
            .method(Method::GET)
            .uri("/widgets/1")
            .header(header::ACCEPT, SIGN1)
            .header(NONCE_HEADER, nonce.to_header_value())
            .body(Body::empty())
            .expect("request");
        match selector {
            Some(digest) => selecting(req, &digest),
            None => req,
        }
    };
    let call = Call::new(Method::GET, "/widgets/{id}", &["1"]);
    let digest = request_digest_unsigned(&nonce, &[]);
    // A selector naming an accepted digest picks it, and none falls back to
    // the op's current digest: an unsigned request proves nothing.
    for (selector, sealed_under) in [(Some(OLD), OLD), (Some(NEW), NEW), (None, NEW)] {
        let answer = send(&router, get(selector)).await;
        assert_eq!(answer.status, StatusCode::OK);
        call.clone_with_contract(sealed_under)
            .open(digest, answer.status, answer.body)
            .await
            .unwrap_or_else(|error| panic!("{selector:?}: {error}"));
    }
    // A well-formed selector naming nothing is the same unsigned 426 the
    // signed path answers, not a response sealed under a digest the client
    // cannot open.
    let answer = send(&router, get(Some([0x55; 32]))).await;
    assert_eq!(answer.status, StatusCode::UPGRADE_REQUIRED);
    assert!(!answer.is_sealed());
}
