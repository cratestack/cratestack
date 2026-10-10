//! Every fail-closed refusal of a payload type over REST (cratestack#1168):
//! unsigned and before any key, a lying header, a replay.

use cratestack_core::{
    PAYLOAD_ACCEPT_HEADER, PAYLOAD_TYPE_HEADER, PAYLOAD_TYPE_NOT_ACCEPTABLE_REST_CODE,
    PAYLOAD_TYPE_UNSUPPORTED_REST_CODE,
};
use http::{HeaderValue, Method, StatusCode};

use super::contracts_counting::rest_counting;
use super::fixtures::Hits;
use super::payload_types_support::*;
use super::support::*;

const FORM_BODY: &[u8] = b"amount=1500&currency=xaf";

#[tokio::test]
async fn a_request_type_the_layer_or_the_route_does_not_accept_is_an_unsigned_415_before_any_key_is_used()
 {
    // The default layer takes CBOR alone; the opted-in one is narrowed by the
    // route (`/pay` takes only a form).
    let hits = Hits::default();
    let (default_layer, counts) = rest_counting(CONTRACTS, None, &hits);
    let call = Call::new(Method::POST, "/widgets", &[]).types(JSON, CBOR);
    let sealed = call.seal(b"{}").await;
    let cases = [
        (default_layer, "/widgets", JSON),
        (router(&hits), "/pay", JSON),
        (router(&hits), "/pay", CBOR),
        (router(&hits), "/pay", "application/cose"),
        (router(&hits), "/pay", "application/cbor-seq"),
    ];
    for (router, uri, request_type) in cases {
        let answer = send(
            &router,
            typed_request(Method::POST, uri, sealed.clone(), Some(request_type), None),
        )
        .await;
        assert_eq!(
            answer.status,
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "{uri} {request_type}"
        );
        assert!(!answer.is_sealed(), "unsigned");
        assert_eq!(
            error_code(&answer.body),
            PAYLOAD_TYPE_UNSUPPORTED_REST_CODE,
            "{uri} {request_type}"
        );
    }
    assert_eq!(hits.get(), 0);
    assert_eq!(counts.resolves(), 0, "no key was looked up");
    assert_eq!(
        counts.passes(),
        0,
        "nothing was opened, so no nonce was spent"
    );
}

#[tokio::test]
async fn no_acceptable_response_type_is_an_unsigned_406_before_any_key_is_used() {
    let hits = Hits::default();
    let (default_layer, counts) = rest_counting(CONTRACTS, None, &hits);
    let sealed = Call::new(Method::POST, "/widgets", &[]).seal(PAYLOAD).await;
    // The default layer answers CBOR alone; `/pay` answers JSON alone; and a
    // type the layer does not allow (or may never seal) is no answer at all.
    let cases = [
        (default_layer, "/widgets", None, JSON),
        (router(&hits), "/pay", Some(FORM), CBOR),
        (router(&hits), "/pay", Some(FORM), "text/html"),
        (router(&hits), "/pay", Some(FORM), "application/cose"),
        (router(&hits), "/either", None, "text/html"),
    ];
    for (router, uri, request_type, accept) in cases {
        let answer = send(
            &router,
            typed_request(
                Method::POST,
                uri,
                sealed.clone(),
                request_type,
                Some(accept),
            ),
        )
        .await;
        assert_eq!(answer.status, StatusCode::NOT_ACCEPTABLE, "{uri} {accept}");
        assert!(!answer.is_sealed(), "unsigned");
        assert_eq!(
            error_code(&answer.body),
            PAYLOAD_TYPE_NOT_ACCEPTABLE_REST_CODE,
            "{uri} {accept}"
        );
    }
    assert_eq!(hits.get(), 0);
    assert_eq!(counts.resolves(), 0, "no key was looked up");
    assert_eq!(
        counts.passes(),
        0,
        "nothing was opened, so no nonce was spent"
    );
}

#[tokio::test]
async fn a_selector_header_sent_twice_or_malformed_is_an_unsigned_400() {
    let hits = Hits::default();
    let sealed = Call::new(Method::POST, "/pay", &[])
        .types(FORM, JSON)
        .seal(FORM_BODY)
        .await;
    let malformed = [
        (PAYLOAD_TYPE_HEADER, vec!["Application/Json"]),
        (PAYLOAD_TYPE_HEADER, vec!["application/json; charset=utf-8"]),
        (PAYLOAD_TYPE_HEADER, vec![FORM, FORM]),
        (PAYLOAD_TYPE_HEADER, vec![""]),
        (
            PAYLOAD_ACCEPT_HEADER,
            vec!["application/json,application/cbor"],
        ),
        (PAYLOAD_ACCEPT_HEADER, vec!["*/*"]),
        (PAYLOAD_ACCEPT_HEADER, vec!["application/json;q=0.5"]),
        (PAYLOAD_ACCEPT_HEADER, vec![JSON, JSON]),
    ];
    for (name, values) in malformed {
        let mut req = typed_request(Method::POST, "/pay", sealed.clone(), Some(FORM), Some(JSON));
        req.headers_mut().remove(name);
        for value in &values {
            req.headers_mut()
                .append(name, HeaderValue::from_str(value).expect("header"));
        }
        let answer = send(&router(&hits), req).await;
        assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{name} {values:?}");
        assert!(!answer.is_sealed());
    }
    assert_eq!(hits.get(), 0);
}

#[tokio::test]
async fn a_header_that_lies_about_the_payload_type_is_the_coarse_401() {
    let hits = Hits::default();
    let router = router(&hits);
    // Sealed as a form request, but the header says JSON (allowed on
    // `/either`): the AAD the server rebuilds names JSON, so it fails.
    let call = Call::new(Method::POST, "/either", &[]).types(FORM, JSON);
    let sealed = call.seal(FORM_BODY).await;
    let lied = send(
        &router,
        typed_request(Method::POST, "/either", sealed, Some(JSON), Some(JSON)),
    )
    .await;
    assert_eq!(lied.status, StatusCode::UNAUTHORIZED);
    assert!(!lied.is_sealed());

    // Sealed as CBOR (no header) but the header says JSON.
    let sealed = Call::new(Method::POST, "/either", &[]).seal(PAYLOAD).await;
    let lied = send(
        &router,
        typed_request(Method::POST, "/either", sealed, Some(JSON), None),
    )
    .await;
    assert_eq!(lied.status, StatusCode::UNAUTHORIZED);

    // Sealed as JSON but the header is silent, so CBOR is assumed.
    let sealed = Call::new(Method::POST, "/either", &[])
        .types(JSON, CBOR)
        .seal(b"{}")
        .await;
    let silent = send(
        &router,
        typed_request(Method::POST, "/either", sealed, None, None),
    )
    .await;
    assert_eq!(silent.status, StatusCode::UNAUTHORIZED);

    // One coarse 401, whatever was wrong.
    assert_eq!(lied.body, silent.body);
    assert_eq!(hits.get(), 0, "no handler ran");
}
