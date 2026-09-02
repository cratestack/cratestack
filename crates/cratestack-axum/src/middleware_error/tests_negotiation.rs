//! Content negotiation must never rewrite the status (cratestack#846
//! security review, finding 3). `Accept` is caller-controlled, so a
//! negotiation failure that replaced the status let any caller downgrade
//! its own throttle to a 406 or a 400.

use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use cratestack_codec_cbor::CborCodec;
use cratestack_core::{CratestackCodec, CratestackError, CratestackErrorResponse};

use super::middleware_error_response;
use super::tests::{headers_with_accept, parts};

/// An `Accept` this layer cannot satisfy must not turn a throttle into a
/// `406`. RFC 9110 §12.5.1 permits an origin server to disregard `Accept`
/// and send a representation anyway, which is strictly more useful here:
/// a 406 would discard the fact that the caller was throttled at all.
#[tokio::test]
async fn an_unsatisfiable_accept_keeps_the_status_and_falls_back_to_the_default_codec() {
    let response = middleware_error_response(
        &headers_with_accept(Some("text/html")),
        "/transfer",
        CratestackError::TooManyRequests("rate limit exceeded".to_owned()),
    );
    let (status, content_type, body) = parts(response).await;

    assert_eq!(
        status,
        StatusCode::TOO_MANY_REQUESTS,
        "negotiation must not rewrite a 429 into a 406 — that is caller-triggerable and          loses the throttle"
    );
    assert_eq!(content_type, "application/cbor");
    let decoded: CratestackErrorResponse = CborCodec.decode(&body).expect("typed envelope");
    assert_eq!(decoded.code, "TOO_MANY_REQUESTS");
}

/// Same for a malformed `Accept`, which used to produce a `400`.
#[tokio::test]
async fn a_malformed_accept_keeps_the_status_too() {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::ACCEPT,
        HeaderValue::from_bytes(&[0xff, 0xfe]).expect("a non-UTF-8 header value"),
    );

    let response = middleware_error_response(
        &headers,
        "/transfer",
        CratestackError::TooManyRequests("rate limit exceeded".to_owned()),
    );
    let (status, content_type, body) = parts(response).await;

    assert_eq!(
        status,
        StatusCode::TOO_MANY_REQUESTS,
        "a caller cannot downgrade its own throttle to a 400 by sending header junk"
    );
    assert_eq!(content_type, "application/cbor");
    let decoded: CratestackErrorResponse = CborCodec.decode(&body).expect("typed envelope");
    assert_eq!(decoded.code, "TOO_MANY_REQUESTS");
}

/// A wildcard `Accept` resolves to the default, not to a 406.
#[tokio::test]
async fn a_wildcard_accept_resolves_to_the_default_codec() {
    let response = middleware_error_response(
        &headers_with_accept(Some("*/*")),
        "/transfer",
        CratestackError::TooManyRequests("rate limit exceeded".to_owned()),
    );
    let (status, content_type, body) = parts(response).await;

    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(content_type, "application/cbor");
    let decoded: CratestackErrorResponse = CborCodec.decode(&body).expect("typed envelope");
    assert_eq!(decoded.code, "TOO_MANY_REQUESTS");
}

/// A missing `Accept` is the no-preference case and takes the default —
/// covered above for a 429, asserted here for a 5xx too so the status
/// passthrough is not accidentally specific to one code.
#[tokio::test]
async fn a_missing_accept_keeps_a_5xx_status() {
    let response = middleware_error_response(
        &headers_with_accept(None),
        "/transfer",
        CratestackError::Unavailable("rate limit store temporarily unavailable".to_owned()),
    );
    let (status, content_type, body) = parts(response).await;

    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(content_type, "application/cbor");
    let decoded: CratestackErrorResponse = CborCodec.decode(&body).expect("typed envelope");
    assert_eq!(decoded.code, "UNAVAILABLE");
}
