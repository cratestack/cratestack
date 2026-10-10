//! A payload that is not empty is checked on a read or delete too
//! (cratestack#1168); see `payload_types_reads` for the empty one.

use cratestack_core::PAYLOAD_TYPE_UNSUPPORTED_REST_CODE;
use cratestack_cose::request_digest;
use http::{Method, StatusCode};

use super::contracts_counting::rest_counting;
use super::fixtures::Hits;
use super::payload_types_support::*;
use super::support::*;

#[tokio::test]
async fn a_payload_that_is_not_empty_is_checked_even_on_a_read() {
    // The payload's type must be one the layer allows (CBOR alone by
    // default), and one the route allows when it declares any: a refusal
    // sealed under the binding it was sent with, before the handler runs.
    let hits = Hits::default();
    let (default_layer, _) = rest_counting(CONTRACTS, None, &hits);
    let cases = [
        (default_layer, "/notes", JSON, None, CBOR),
        (router(&hits), "/form-read", JSON, Some(JSON), JSON),
        (router(&hits), "/form-read", CBOR, Some(JSON), JSON),
    ];
    for (router, uri, request_type, accept, response_type) in cases {
        let call = Call::new(Method::GET, uri, &[]).types(request_type, response_type);
        let sealed = call.seal(b"{}").await;
        let answer = send(
            &router,
            typed_request(Method::GET, uri, sealed.clone(), Some(request_type), accept),
        )
        .await;
        assert_eq!(
            answer.status,
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "{uri} {request_type}"
        );
        assert!(answer.is_sealed(), "{uri} {request_type}");
        let body = call
            .open(request_digest(&sealed), answer.status, answer.body)
            .await
            .expect("the refusal is sealed under the request's binding");
        assert!(!body.is_empty());
    }
    assert_eq!(hits.get(), 0, "the handler never ran");

    // Unchanged from 0.15.3: a CBOR payload on a read still reaches it.
    let call = Call::new(Method::GET, "/notes", &[]);
    let sealed = call.seal(PAYLOAD).await;
    let (default_layer, _) = rest_counting(CONTRACTS, None, &hits);
    let answer = send(
        &default_layer,
        typed_request(Method::GET, "/notes", sealed, None, None),
    )
    .await;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(hits.get(), 1);
    // And the pre-open refusal of a POST is not affected.
    let post = Call::new(Method::POST, "/widgets", &[]).types(JSON, CBOR);
    let sealed = post.seal(b"{}").await;
    let answer = send(
        &router(&hits),
        typed_request(Method::POST, "/pay", sealed, Some(CBOR), None),
    )
    .await;
    assert_eq!(answer.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert!(!answer.is_sealed());
    assert_eq!(error_code(&answer.body), PAYLOAD_TYPE_UNSUPPORTED_REST_CODE);
}
