//! Binding version 2 at the layer (cratestack#1123, EXT-14): the AAD binds
//! the digest of the op called, the unbound `Cratestack-Contract` header
//! picks which accepted digest to verify under, and the response is sealed
//! under the digest the request used. Several accepted digests for one op
//! are `contracts_history.rs`.

use std::sync::atomic::Ordering;

use cratestack_core::{AcceptedContracts, CONTRACT_HEADER, request_digest};
use http::{HeaderValue, Method, StatusCode};

use super::contracts_support::*;
use super::fixtures::{Hits, REST_ROUTES};
use super::support::*;
use crate::envelope_layer::{EnvelopeLayer, EnvelopeMode};

#[tokio::test]
async fn a_header_selected_request_is_opened_once_however_long_the_history() {
    let hits = Hits::default();
    let (_, _, req) = post_widgets(OLD, Some(OLD)).await;
    let (router, opens) = rest_counting(WITH_HISTORY, None, &hits);
    let answer = send(&router, req).await;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(opens.load(Ordering::SeqCst), 1, "one verification, not two");
}

#[tokio::test]
async fn the_current_digest_opens_and_the_response_is_sealed_under_it() {
    let hits = Hits::default();
    let (call, sealed, req) = post_widgets(NEW, Some(NEW)).await;
    let answer = send(&rest(WITH_HISTORY, None, &hits), req).await;
    assert_eq!(answer.status, StatusCode::OK);
    assert!(answer.is_sealed());
    call.open(request_digest(&sealed), answer.status, answer.body)
        .await
        .expect("verifies under the digest the request used");
}

#[tokio::test]
async fn a_selector_naming_no_accepted_digest_is_the_unsigned_426_before_any_key_lookup() {
    let hits = Hits::default();
    let stale = [0x44; 32];
    let (_, _, req) = post_widgets(stale, Some(stale)).await;
    let (router, opens) = rest_counting(WITH_HISTORY, None, &hits);
    let answer = send(&router, req).await;
    assert_eq!(answer.status, StatusCode::UPGRADE_REQUIRED);
    assert!(!answer.is_sealed(), "unsigned: a hint, never proof");
    assert_eq!(error_code(&answer.body), "CONTRACT_UNSUPPORTED");
    assert_eq!(hits.get(), 0);
    assert_eq!(opens.load(Ordering::SeqCst), 0, "no key was looked up");
}

#[tokio::test]
async fn the_426_uses_the_rpc_vocabulary_on_an_rpc_path() {
    let hits = Hits::default();
    let call = Call::new(Method::POST, "procedure.notify", &[]).contract([0x44; 32]);
    let sealed = call.seal(PAYLOAD).await;
    let req = selecting(
        cose_request(Method::POST, "/rpc/procedure.notify", sealed),
        &[0x44; 32],
    );
    let answer = send(&rpc(&hits), req).await;
    assert_eq!(answer.status, StatusCode::UPGRADE_REQUIRED);
    assert!(!answer.is_sealed());
    assert_eq!(error_code(&answer.body), "contract_unsupported");
    assert_eq!(hits.get(), 0);
}

#[tokio::test]
async fn a_stale_digest_without_the_selector_is_the_coarse_401() {
    let hits = Hits::default();
    let (_, _, req) = post_widgets([0x44; 32], None).await;
    let answer = send(&rest(WITH_HISTORY, None, &hits), req).await;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED);
    assert_eq!(hits.get(), 0);
}

#[tokio::test]
async fn a_lie_in_the_selector_is_a_401_not_a_426() {
    let hits = Hits::default();
    // Sealed under a digest the server does not accept, selecting one it
    // does: the header is unbound and cannot vouch for the AAD.
    let (_, _, req) = post_widgets([0x44; 32], Some(NEW)).await;
    let answer = send(&rest(WITH_HISTORY, None, &hits), req).await;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED);
    assert_eq!(hits.get(), 0);
}

#[tokio::test]
async fn another_ops_digest_is_refused_even_with_a_matching_route_and_selector() {
    let hits = Hits::default();
    // `POST /widgets` sealed under `GET /widgets/{id}`'s digest.
    let (_, _, req) = post_widgets(OTHER_OP, Some(OTHER_OP)).await;
    let answer = send(&rest(WITH_HISTORY, None, &hits), req).await;
    assert_eq!(answer.status, StatusCode::UPGRADE_REQUIRED);
    let (_, _, unselected) = post_widgets(OTHER_OP, None).await;
    let answer = send(&rest(WITH_HISTORY, None, &hits), unselected).await;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED);
    assert_eq!(hits.get(), 0);
}

#[tokio::test]
async fn a_malformed_or_repeated_selector_is_a_400() {
    let hits = Hits::default();
    let (router, opens) = rest_counting(WITH_HISTORY, None, &hits);
    for values in [&["not-a-selector"][..], &["AAAAAAAAAAA", "AAAAAAAAAAA"][..]] {
        let (_, _, mut req) = post_widgets(NEW, None).await;
        for value in values {
            req.headers_mut()
                .append(CONTRACT_HEADER, HeaderValue::from_static(value));
        }
        let answer = send(&router, req).await;
        assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{values:?}");
    }
    assert_eq!(hits.get(), 0);
    assert_eq!(opens.load(Ordering::SeqCst), 0, "refused before any open");
}

#[tokio::test]
async fn a_route_with_no_contract_row_is_a_500_for_a_signed_request() {
    static EMPTY: AcceptedContracts = &[("GET /widgets/{id}", &[NEW])];
    let hits = Hits::default();
    let (_, _, req) = post_widgets(NEW, Some(NEW)).await;
    let (router, opens) = rest_counting(EMPTY, None, &hits);
    let answer = send(&router, req).await;
    assert_eq!(answer.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(!answer.is_sealed());
    assert_eq!(hits.get(), 0);
    assert_eq!(opens.load(Ordering::SeqCst), 0, "refused before any open");
}

#[tokio::test]
async fn a_head_is_served_under_the_get_ops_digest() {
    let hits = Hits::default();
    let call = Call::new(Method::HEAD, "/widgets/{id}", &["1"]).contract(OTHER_OP);
    let sealed = call.seal(&[]).await;
    let req = selecting(cose_request(Method::HEAD, "/widgets/1", sealed), &OTHER_OP);
    let answer = send(&rest(WITH_HISTORY, None, &hits), req).await;
    assert_eq!(answer.status, StatusCode::OK);
}

#[tokio::test]
async fn a_signed_batch_binds_its_own_row() {
    let hits = Hits::default();
    let body = batch_frames(&["procedure.notify"]);
    for (digest, expected) in [
        (NEW, StatusCode::OK),
        (OTHER_OP, StatusCode::UPGRADE_REQUIRED),
    ] {
        let call = Call::new(Method::POST, "batch", &[]).contract(digest);
        let req = selecting(
            cose_request(Method::POST, "/rpc/batch", call.seal(&body).await),
            &digest,
        );
        assert_eq!(send(&rpc(&hits), req).await.status, expected);
    }
}

#[tokio::test]
async fn an_unresolvable_contract_count_is_refused_at_build() {
    let error = EnvelopeLayer::builder(server_envelope(), AUDIENCE, WITH_HISTORY)
        .policy(EnvelopeMode::Required)
        .rest("", &REST_ROUTES)
        .max_contract_trials(0)
        .build()
        .expect_err("zero trials can open nothing");
    assert!(error.to_string().contains("contract trials"), "{error}");
}
