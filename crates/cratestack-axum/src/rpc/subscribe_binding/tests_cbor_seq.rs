//! The `application/cbor-seq` half of subscribe negotiation and serving
//! (cratestack#1001); the SSE table lives in `tests.rs`.

use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use cratestack_core::{CratestackError, SubscriptionStream};
use futures_util::stream;

use super::*;

/// One `Vary` header, valued `Accept`: a doubled header passes a first-value check.
fn assert_vary_accept_once(response: &axum::response::Response) {
    let vary: Vec<_> = response.headers().get_all(header::VARY).iter().collect();
    assert_eq!(vary.len(), 1, "Vary must appear once: {vary:?}");
    assert_eq!(vary[0], "Accept");
}

fn negotiate(accept: Option<&[u8]>) -> Result<SubscribeBinding, CratestackError> {
    let mut headers = HeaderMap::new();
    if let Some(value) = accept {
        headers.insert(header::ACCEPT, HeaderValue::from_bytes(value).unwrap());
    }
    negotiate_subscribe_binding(&headers)
}

#[test]
fn cbor_seq_is_its_own_binding_and_the_first_known_entry_wins() {
    let cases: &[(&[u8], SubscribeBinding)] = &[
        (b"application/cbor-seq", SubscribeBinding::CborSeq),
        (b" application/cbor-seq ", SubscribeBinding::CborSeq),
        (
            b"application/json, application/cbor-seq",
            SubscribeBinding::CborSeq,
        ),
        (
            b"application/cbor-seq, text/event-stream",
            SubscribeBinding::CborSeq,
        ),
        (b"application/cbor-seq, */*", SubscribeBinding::CborSeq),
        (
            b"text/event-stream, application/cbor-seq",
            SubscribeBinding::Sse,
        ),
        (b"*/*, application/cbor-seq", SubscribeBinding::Sse),
    ];
    for (accept, want) in cases {
        assert_eq!(negotiate(Some(accept)).unwrap(), *want, "{accept:?}");
    }
    for accept in [
        &b"application/cbor-seq;q=0"[..],
        b"application/cbor-seq; q=0.5",
        b"application/cbor",
        b"APPLICATION/CBOR-SEQ",
        b"application/*",
    ] {
        let error = negotiate(Some(accept)).unwrap_err();
        assert!(
            matches!(error, CratestackError::NotAcceptable(_)),
            "{accept:?}"
        );
    }
}

#[test]
fn each_binding_hands_the_dispatcher_its_own_codec() {
    assert!(matches!(
        SubscribeBinding::Sse.codec(),
        SubscribeCodec::Json(_)
    ));
    assert!(matches!(
        SubscribeBinding::CborSeq.codec(),
        SubscribeCodec::Cbor(_)
    ));
}

#[test]
#[allow(deprecated)]
fn the_deprecated_wrapper_stays_sse_only() {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::ACCEPT,
        HeaderValue::from_static("application/cbor-seq"),
    );
    assert!(validate_subscribe_accept_header(&headers).is_err());
}

#[tokio::test]
async fn serve_subscription_streams_an_opened_subscription_as_cbor_seq() {
    let opened = async { Ok(Box::pin(stream::iter([Ok(1_i64)])) as SubscriptionStream<i64>) };
    let response = serve_subscription(SubscribeBinding::CborSeq, opened).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/cbor-seq"
    );
    assert_vary_accept_once(&response);
}

#[tokio::test]
async fn both_bindings_and_a_refusal_vary_on_accept() {
    for binding in [SubscribeBinding::Sse, SubscribeBinding::CborSeq] {
        let opened = async { Ok(Box::pin(stream::iter([Ok(1_i64)])) as SubscriptionStream<i64>) };
        let ok = serve_subscription(binding, opened).await;
        assert_vary_accept_once(&ok);
        let error = CratestackError::Forbidden("no".to_owned());
        let refused = serve_subscription::<i64, _>(binding, async { Err(error) }).await;
        assert_vary_accept_once(&refused);
    }
}

#[tokio::test]
async fn an_open_failure_is_the_json_refusal_on_either_binding() {
    for binding in [SubscribeBinding::Sse, SubscribeBinding::CborSeq] {
        let error = CratestackError::Forbidden("not yours".to_owned());
        let got = serve_subscription::<i64, _>(binding, async { Err(error) }).await;
        assert_eq!(got.status(), StatusCode::FORBIDDEN);
        assert_eq!(
            got.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/json"
        );
    }
}
