use axum::http::HeaderValue;
use futures_util::stream;

use super::*;

#[derive(Serialize)]
struct Widget {
    id: i64,
}

async fn body_to_string(response: Response) -> String {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[test]
fn validate_subscribe_accept_header_accepts_exact_match() {
    let mut headers = HeaderMap::new();
    headers.insert(header::ACCEPT, HeaderValue::from_static(SSE_CONTENT_TYPE));
    assert!(validate_subscribe_accept_header(&headers).is_ok());
}

#[test]
fn validate_subscribe_accept_header_accepts_wildcard() {
    let mut headers = HeaderMap::new();
    headers.insert(header::ACCEPT, HeaderValue::from_static("*/*"));
    assert!(validate_subscribe_accept_header(&headers).is_ok());
}

#[test]
fn validate_subscribe_accept_header_rejects_missing_header() {
    let headers = HeaderMap::new();
    let error = validate_subscribe_accept_header(&headers).unwrap_err();
    assert!(matches!(error, CoolError::NotAcceptable(_)));
}

#[test]
fn validate_subscribe_accept_header_rejects_mismatched_type() {
    let mut headers = HeaderMap::new();
    headers.insert(header::ACCEPT, HeaderValue::from_static("application/json"));
    let error = validate_subscribe_accept_header(&headers).unwrap_err();
    assert!(matches!(error, CoolError::NotAcceptable(_)));
}

#[tokio::test]
async fn encodes_one_message_event_per_item_with_incrementing_ids() {
    let items = stream::iter([Widget { id: 1 }, Widget { id: 2 }]);
    let response = encode_model_event_sse_response(items);
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        SSE_CONTENT_TYPE,
    );

    let body = body_to_string(response).await;
    assert_eq!(
        body,
        "event: message\ndata: {\"id\":1,\"next\":{\"id\":1}}\n\n\
         event: message\ndata: {\"id\":2,\"next\":{\"id\":2}}\n\n\
         event: error\ndata: {\"id\":3,\"err\":{\"code\":\"unavailable\",\"message\":\"subscription lagged\"}}\n\n"
    );
}

#[tokio::test]
async fn empty_stream_still_ends_with_a_terminal_error_event() {
    let items: stream::Iter<std::vec::IntoIter<Widget>> = stream::iter(Vec::new());
    let response = encode_model_event_sse_response(items);
    let body = body_to_string(response).await;
    assert_eq!(
        body,
        "event: error\ndata: {\"id\":1,\"err\":{\"code\":\"unavailable\",\"message\":\"subscription lagged\"}}\n\n"
    );
}

#[test]
fn lagged_error_is_stable_and_maps_to_unavailable_code() {
    let error = lagged_error();
    assert_eq!(error.code, "unavailable");
    assert_eq!(error.message, "subscription lagged");
}
