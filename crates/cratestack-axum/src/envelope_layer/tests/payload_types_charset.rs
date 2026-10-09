//! A handler's `charset` is not dropped silently (cratestack#1168): the
//! sealed type carries no parameters and a client decodes JSON as UTF-8, so
//! a body labelled with another charset is not sealed as the negotiated type.

use cratestack_cose::request_digest;
use http::{Method, StatusCode};

use super::fixtures::Hits;
use super::payload_types_support::*;
use super::support::*;

#[tokio::test]
async fn a_success_labelled_with_another_charset_is_a_sealed_500_not_json() {
    let hits = Hits::default();
    let call = Call::new(Method::GET, "/utf16", &[]).types(CBOR, JSON);
    let sealed = call.seal(&[]).await;
    let answer = send(
        &router(&hits),
        typed_request(Method::GET, "/utf16", sealed.clone(), None, Some(JSON)),
    )
    .await;
    assert_eq!(answer.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(answer.is_sealed());
    let payload = call
        .open(request_digest(&sealed), answer.status, answer.body)
        .await
        .expect("verifies");
    assert_eq!(json_error_code(&payload), "INTERNAL_ERROR");
}
