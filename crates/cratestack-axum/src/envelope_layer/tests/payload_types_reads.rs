//! A request with no payload (cratestack#1168 follow-up): a signed `GET`,
//! `HEAD` or `DELETE` against the routes a schema generates is exactly what
//! 0.15.3 served, and the type an empty payload names is bound as sent but
//! never checked against the route's request types, which a generated read
//! declares empty (`request_types: &[]`, "no constraint" everywhere else).
//! A payload that is not empty is always checked, before the handler.

use cratestack_cose::request_digest;
use http::{Method, StatusCode};

use super::contracts_counting::rest_counting;
use super::fixtures::Hits;
use super::payload_types_support::*;
use super::support::*;

fn call(method: Method, route: &'static str, params: &[&'static str]) -> Call {
    Call::new(method, route, params)
}

#[tokio::test]
async fn a_signed_read_or_delete_with_no_selector_header_is_served_as_in_0_15_3() {
    let hits = Hits::default();
    let (router, _) = rest_counting(CONTRACTS, None, &hits);
    let reads = [
        (
            Method::GET,
            "/widgets/7",
            call(Method::GET, "/widgets/{id}", &["7"]),
        ),
        (
            Method::HEAD,
            "/widgets/7",
            call(Method::HEAD, "/widgets/{id}", &["7"]),
        ),
        (
            Method::DELETE,
            "/widgets/7",
            call(Method::DELETE, "/widgets/{id}", &["7"]),
        ),
        (
            Method::GET,
            "/notes?limit=5",
            Call {
                query: Some("limit=5"),
                ..call(Method::GET, "/notes", &[])
            },
        ),
    ];
    for (method, uri, call) in reads {
        let sealed = call.seal(&[]).await;
        let answer = send(
            &router,
            typed_request(method.clone(), uri, sealed.clone(), None, None),
        )
        .await;
        assert_eq!(answer.status, StatusCode::OK, "{method} {uri}");
        assert!(answer.is_sealed(), "{method} {uri}");
        if method != Method::HEAD {
            call.open(request_digest(&sealed), answer.status, answer.body.clone())
                .await
                .expect("a client binding application/cbor verifies the answer");
        }
    }
    assert_eq!(hits.get(), 4);
}

#[tokio::test]
async fn the_type_an_empty_payload_names_is_bound_as_sent_and_never_refused() {
    // CBOR named outright is the same as saying nothing; a JSON or form
    // client's bodiless call names its own codec, which the route (empty or
    // form-only) and the default layer (CBOR alone) need not list.
    // (request type, uri, route, id, accept, response type, opted-in layer)
    let cases = [
        (
            CBOR,
            "/widgets/7",
            "/widgets/{id}",
            Some("7"),
            None,
            CBOR,
            false,
        ),
        (
            JSON,
            "/widgets/7",
            "/widgets/{id}",
            Some("7"),
            None,
            CBOR,
            false,
        ),
        (
            FORM,
            "/widgets/7",
            "/widgets/{id}",
            Some("7"),
            None,
            CBOR,
            false,
        ),
        (
            JSON,
            "/form-read",
            "/form-read",
            None,
            Some(JSON),
            JSON,
            true,
        ),
    ];
    for (request_type, uri, route, id, accept, response_type, opted_in) in cases {
        let hits = Hits::default();
        let router = match opted_in {
            true => router(&hits),
            false => rest_counting(CONTRACTS, None, &hits).0,
        };
        let params: Vec<&'static str> = id.into_iter().collect();
        let call = call(Method::GET, route, &params).types(request_type, response_type);
        let sealed = call.seal(&[]).await;
        let answer = send(
            &router,
            typed_request(Method::GET, uri, sealed.clone(), Some(request_type), accept),
        )
        .await;
        assert_eq!(answer.status, StatusCode::OK, "{uri} {request_type}");
        call.open(request_digest(&sealed), answer.status, answer.body.clone())
            .await
            .expect("verifies under the type the request bound");
        // The handler saw a plain GET with no payload and no content type.
        assert_eq!(answer.seen("x-seen-content-type"), "<absent>");
        assert_eq!(hits.get(), 1);
    }
}

#[tokio::test]
async fn a_type_the_signer_did_not_bind_still_fails_verification_on_an_empty_payload() {
    // Not checked is not unbound: a header that disagrees with what was
    // signed is the coarse, unsigned 401.
    let hits = Hits::default();
    let (router, _) = rest_counting(CONTRACTS, None, &hits);
    let sealed = call(Method::GET, "/widgets/{id}", &["7"]).seal(&[]).await;
    let answer = send(
        &router,
        typed_request(Method::GET, "/widgets/7", sealed, Some(JSON), None),
    )
    .await;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED);
    assert!(!answer.is_sealed());
    assert_eq!(hits.get(), 0);
}
