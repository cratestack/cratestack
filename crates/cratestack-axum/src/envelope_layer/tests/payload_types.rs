//! Negotiated, AAD-bound payload types over REST (cratestack#1168): a form
//! in and JSON out inside the seal (vpay's shape), and how a handler's
//! answer in the wrong type is sealed. The refusals are in
//! `payload_types_refusals`.

use bytes::Bytes;
use cratestack_cose::request_digest;
use http::{Method, StatusCode};

use super::fixtures::{Hits, JSON_ANSWER};
use super::payload_types_support::*;
use super::support::*;

const FORM_BODY: &[u8] = b"amount=1500&currency=xaf";

#[tokio::test]
async fn a_form_in_json_out_exchange_is_sealed_and_bound_both_ways() {
    let hits = Hits::default();
    let call = Call::new(Method::POST, "/pay", &[]).types(FORM, JSON);
    let sealed = call.seal(FORM_BODY).await;
    let answer = send(
        &router(&hits),
        typed_request(Method::POST, "/pay", sealed.clone(), Some(FORM), Some(JSON)),
    )
    .await;

    assert_eq!(answer.status, StatusCode::OK);
    assert!(answer.is_sealed(), "{}", answer.content_type());
    // The handler saw an ordinary form POST that asks for JSON, and none of
    // the layer's own headers.
    assert_eq!(answer.seen("x-seen-content-type"), FORM);
    assert_eq!(answer.seen("x-seen-accept"), JSON);
    assert_eq!(answer.seen("x-seen-payload-type"), "<absent>");
    assert_eq!(answer.seen("x-seen-payload-accept"), "<absent>");
    // The response names its own type, and binds it.
    assert_eq!(answer.seen("cratestack-payload-type"), JSON);
    let payload = call
        .open(request_digest(&sealed), answer.status, answer.body.clone())
        .await
        .expect("a client binding JSON for the response verifies");
    assert_eq!(payload.as_ref(), JSON_ANSWER.as_bytes());

    // The same bytes under the request's type do not verify: the response
    // type is in the AAD.
    let wrong = Call::new(Method::POST, "/pay", &[]).types(FORM, CBOR);
    wrong
        .open(request_digest(&sealed), answer.status, answer.body)
        .await
        .expect_err("a response sealed as JSON is not a CBOR response");
    assert_eq!(hits.get(), 1);
}

#[tokio::test]
async fn a_cbor_request_may_ask_for_json_and_a_json_request_for_cbor() {
    for (request_type, accept, response_type) in [(CBOR, JSON, JSON), (JSON, CBOR, CBOR)] {
        let hits = Hits::default();
        let call = Call::new(Method::POST, "/either", &[]).types(request_type, response_type);
        let sealed = call.seal(b"{}").await;
        let answer = send(
            &router(&hits),
            typed_request(
                Method::POST,
                "/either",
                sealed.clone(),
                Some(request_type),
                Some(accept),
            ),
        )
        .await;
        assert_eq!(answer.status, StatusCode::OK, "{request_type} -> {accept}");
        assert_eq!(answer.seen("x-seen-content-type"), request_type);
        assert_eq!(answer.seen("cratestack-payload-type"), response_type);
        call.open(request_digest(&sealed), answer.status, answer.body)
            .await
            .expect("verifies under the response's own type");
    }
}

#[tokio::test]
async fn the_clients_order_of_preference_decides_the_response_type() {
    let hits = Hits::default();
    let call = Call::new(Method::POST, "/either", &[]).types(CBOR, JSON);
    let sealed = call.seal(PAYLOAD).await;
    let answer = send(
        &router(&hits),
        typed_request(
            Method::POST,
            "/either",
            sealed.clone(),
            None,
            Some("application/json, application/cbor"),
        ),
    )
    .await;
    assert_eq!(
        answer.seen("x-seen-accept"),
        "application/json, application/cbor"
    );
    assert_eq!(answer.seen("cratestack-payload-type"), JSON);
}

#[tokio::test]
async fn a_success_in_a_type_that_was_not_negotiated_is_a_sealed_500() {
    let hits = Hits::default();
    let call = Call::new(Method::GET, "/html", &[]).types(CBOR, JSON);
    let sealed = call.seal(&[]).await;
    let answer = send(
        &router(&hits),
        typed_request(Method::GET, "/html", sealed.clone(), None, Some(JSON)),
    )
    .await;
    assert_eq!(answer.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(answer.is_sealed(), "sealed, not passed through");
    // The error is sealed in a type the client asked for.
    assert_eq!(answer.seen("cratestack-payload-type"), JSON);
    let payload = call
        .open(request_digest(&sealed), answer.status, answer.body)
        .await
        .expect("verifies");
    assert!(
        !payload.starts_with(b"<p>"),
        "the handler's body is not shown"
    );
    assert_eq!(json_error_code(&payload), "INTERNAL_ERROR");
}

#[tokio::test]
async fn an_error_in_a_foreign_type_is_re_encoded_in_a_negotiated_one_and_sealed() {
    let hits = Hits::default();
    for (accept, response_type) in [
        (JSON, JSON),
        (CBOR, CBOR),
        ("application/json, application/cbor", JSON),
    ] {
        let call = Call::new(Method::GET, "/plain-error", &[]).types(CBOR, response_type);
        let sealed = call.seal(&[]).await;
        let answer = send(
            &router(&hits),
            typed_request(
                Method::GET,
                "/plain-error",
                sealed.clone(),
                None,
                Some(accept),
            ),
        )
        .await;
        assert_eq!(answer.status, StatusCode::NOT_FOUND, "{accept}");
        assert!(answer.is_sealed());
        assert_eq!(answer.seen("cratestack-payload-type"), response_type);
        let payload = call
            .open(request_digest(&sealed), answer.status, answer.body)
            .await
            .expect("verifies");
        let code = match response_type {
            JSON => json_error_code(&payload),
            _ => error_code(&payload),
        };
        assert_eq!(code, "NOT_FOUND");
    }
}

#[tokio::test]
async fn a_layer_error_is_sealed_in_the_negotiated_type_not_forced_to_cbor() {
    // A streamed answer cannot be sealed: the layer replaces it with a
    // sealed 406 of its own, and that error is the client's JSON.
    let hits = Hits::default();
    let call = Call::new(Method::GET, "/stream", &[]).types(CBOR, JSON);
    let sealed = call.seal(&[]).await;
    let answer = send(
        &router(&hits),
        typed_request(Method::GET, "/stream", sealed.clone(), None, Some(JSON)),
    )
    .await;
    assert_eq!(answer.status, StatusCode::NOT_ACCEPTABLE);
    assert_eq!(answer.seen("cratestack-payload-type"), JSON);
    let payload = call
        .open(request_digest(&sealed), answer.status, answer.body)
        .await
        .expect("verifies under JSON");
    assert_eq!(json_error_code(&payload), "NOT_ACCEPTABLE");
}

#[tokio::test]
async fn a_replayed_request_is_still_refused() {
    let hits = Hits::default();
    let router = router(&hits);
    let sealed: Bytes = Call::new(Method::POST, "/pay", &[])
        .types(FORM, JSON)
        .seal(FORM_BODY)
        .await;
    let req = |sealed: &Bytes| {
        typed_request(Method::POST, "/pay", sealed.clone(), Some(FORM), Some(JSON))
    };
    assert_eq!(send(&router, req(&sealed)).await.status, StatusCode::OK);
    assert_eq!(
        send(&router, req(&sealed)).await.status,
        StatusCode::UNAUTHORIZED
    );
}
