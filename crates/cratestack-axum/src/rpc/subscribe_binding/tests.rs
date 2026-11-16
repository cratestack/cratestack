//! Pins `Accept` negotiation. The SSE rows are the table captured from the
//! pre-#1001 `validate_subscribe_accept_header`; `application/cbor-seq` is
//! the one deliberate widening (cratestack#1001, PR 2), and the 406 message
//! now names it.

use axum::body::to_bytes;
use axum::http::{HeaderValue, StatusCode};
use futures_util::stream;

use super::*;

const NEED: &str =
    "subscription endpoint requires Accept: text/event-stream or application/cbor-seq";
/// What the deprecated SSE-only wrapper still says.
const OLD_NEED: &str = "subscription endpoint requires Accept: text/event-stream";

fn negotiate(accept: Option<&[u8]>) -> Result<SubscribeBinding, CratestackError> {
    let mut headers = HeaderMap::new();
    if let Some(value) = accept {
        headers.insert(header::ACCEPT, HeaderValue::from_bytes(value).unwrap());
    }
    negotiate_subscribe_binding(&headers)
}

#[test]
fn accept_negotiation_table_is_unchanged() {
    let accepted: &[&[u8]] = &[
        b"text/event-stream",
        b"*/*",
        b"application/json, text/event-stream",
        b"application/json,*/*",
        b" text/event-stream ",
    ];
    for accept in accepted {
        assert_eq!(
            negotiate(Some(accept)).unwrap(),
            SubscribeBinding::Sse,
            "{accept:?}"
        );
    }

    let refused: &[&[u8]] = &[
        b"text/*",
        b"application/json",
        b"text/event-stream;q=0",
        b"text/event-stream; q=0.5",
        b"text/event-stream;charset=utf-8",
        b"*/*;q=0",
        b"*/*;q=0.1",
        b"TEXT/EVENT-STREAM",
        b"",
    ];
    for accept in refused {
        let error = negotiate(Some(accept)).unwrap_err();
        let got = String::from_utf8_lossy(accept);
        assert!(matches!(error, CratestackError::NotAcceptable(_)), "{got}");
        assert_eq!(error.status_code(), StatusCode::NOT_ACCEPTABLE);
        assert_eq!(
            error.to_string(),
            format!("not acceptable: {NEED}, got {got}"),
        );
    }
}

#[test]
fn missing_accept_is_406_without_a_got_clause() {
    let error = negotiate(None).unwrap_err();
    assert!(matches!(error, CratestackError::NotAcceptable(_)));
    assert_eq!(error.to_string(), format!("not acceptable: {NEED}"));
}

#[test]
fn non_utf8_accept_is_400() {
    let error = negotiate(Some(b"\xff\xfe")).unwrap_err();
    assert!(matches!(error, CratestackError::BadRequest(_)));
    assert_eq!(
        error.to_string(),
        "bad request: invalid Accept header: failed to convert header to a str",
    );
}

/// The deprecated wrapper gives, row for row, what the pre-#1001
/// `validate_subscribe_accept_header` gave: `Ok(())` or the same error.
#[test]
#[allow(deprecated)]
fn deprecated_validate_wrapper_keeps_the_old_results() {
    let accepted: &[&[u8]] = &[
        b"text/event-stream",
        b"*/*",
        b"application/json, text/event-stream",
        b"application/json,*/*",
        b" text/event-stream ",
    ];
    let refused: &[&[u8]] = &[
        b"text/*",
        b"application/json",
        b"text/event-stream;q=0",
        b"text/event-stream; q=0.5",
        b"text/event-stream;charset=utf-8",
        b"*/*;q=0",
        b"*/*;q=0.1",
        b"TEXT/EVENT-STREAM",
        b"",
    ];
    let check = |accept: Option<&[u8]>| {
        let mut headers = HeaderMap::new();
        if let Some(value) = accept {
            headers.insert(header::ACCEPT, HeaderValue::from_bytes(value).unwrap());
        }
        validate_subscribe_accept_header(&headers)
    };
    for accept in accepted {
        assert!(check(Some(accept)).is_ok(), "{accept:?}");
    }
    for accept in refused {
        let got = String::from_utf8_lossy(accept);
        let error = check(Some(accept)).unwrap_err();
        assert!(matches!(error, CratestackError::NotAcceptable(_)), "{got}");
        assert_eq!(
            error.to_string(),
            format!("not acceptable: {OLD_NEED}, got {got}"),
        );
    }
    let error = check(None).unwrap_err();
    assert!(matches!(error, CratestackError::NotAcceptable(_)));
    assert_eq!(error.to_string(), format!("not acceptable: {OLD_NEED}"));
    let error = check(Some(b"\xff\xfe")).unwrap_err();
    assert!(matches!(error, CratestackError::BadRequest(_)));
}

#[tokio::test]
async fn serve_subscription_encodes_an_open_failure_as_the_subscribe_error() {
    let error = CratestackError::Forbidden("not yours".to_owned());
    let want = encode_subscribe_error(&error);
    let want_status = want.status();
    let want_body = to_bytes(want.into_body(), usize::MAX).await.unwrap();

    let got = serve_subscription::<i64, _>(SubscribeBinding::Sse, async { Err(error) }).await;
    assert_eq!(got.status(), StatusCode::FORBIDDEN);
    assert_eq!(got.status(), want_status);
    assert_eq!(
        got.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/json"
    );
    assert_eq!(
        to_bytes(got.into_body(), usize::MAX).await.unwrap(),
        want_body
    );
    assert!(!want_body.is_empty());
}

#[tokio::test]
async fn serve_subscription_streams_an_opened_subscription_as_sse() {
    let opened = async { Ok(Box::pin(stream::iter([Ok(1_i64)])) as SubscriptionStream<i64>) };
    let response = serve_subscription(SubscribeBinding::Sse, opened).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "text/event-stream"
    );
}
