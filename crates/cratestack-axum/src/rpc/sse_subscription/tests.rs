use axum::body::to_bytes;
use axum::http::header;
use cratestack_core::{CratestackError, SubscriptionStream};
use futures_util::stream;

use super::encode_subscription_sse_response;

async fn body_of(items: Vec<Result<serde_json::Value, CratestackError>>) -> (String, String) {
    let stream: SubscriptionStream<serde_json::Value> = Box::pin(stream::iter(items));
    let response = encode_subscription_sse_response(stream);
    let content_type = response.headers()[header::CONTENT_TYPE]
        .to_str()
        .unwrap()
        .to_owned();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (content_type, String::from_utf8(bytes.to_vec()).unwrap())
}

#[tokio::test]
async fn items_then_the_terminal_error_on_the_existing_wire() {
    let (content_type, body) = body_of(vec![
        Ok(serde_json::json!({"n": 1})),
        Err(CratestackError::Unavailable(
            "subscription lagged".to_owned(),
        )),
        Ok(serde_json::json!({"n": 2})),
    ])
    .await;
    assert_eq!(content_type, "text/event-stream");
    assert_eq!(
        body,
        "event: ready\ndata: {\"id\":1,\"ready\":{}}\n\n\
         event: message\ndata: {\"id\":2,\"next\":{\"n\":1}}\n\n\
         event: error\ndata: {\"id\":3,\"err\":{\"code\":\"unavailable\",\"message\":\"subscription lagged\"}}\n\n",
        "nothing is sent after the terminal error",
    );
}

#[tokio::test]
async fn a_stream_that_ends_sends_nothing_more() {
    let (_, body) = body_of(vec![Ok(serde_json::json!(7))]).await;
    assert_eq!(
        body,
        "event: ready\ndata: {\"id\":1,\"ready\":{}}\n\nevent: message\ndata: {\"id\":2,\"next\":7}\n\n"
    );
}
