use std::sync::Arc;

use cratestack_core::{BackplaneMessage, BackplaneReceiver, CratestackError};
use futures_util::StreamExt;
use futures_util::stream;

use super::{model_events, model_feed};

async fn collect(
    items: Vec<Result<u32, CratestackError>>,
    readable: impl FnMut(&u32) -> Result<bool, CratestackError> + Send + 'static,
) -> Vec<Result<serde_json::Value, CratestackError>> {
    let mut readable = readable;
    model_feed(stream::iter(items), move |item: &u32| {
        let verdict = readable(item);
        async move { verdict }
    })
    .collect()
    .await
}

fn unavailable(message: &str) -> impl Fn(&Result<serde_json::Value, CratestackError>) -> bool + '_ {
    move |item| matches!(item, Err(CratestackError::Unavailable(m)) if m == message)
}

#[tokio::test]
async fn readable_events_pass_and_the_rest_are_dropped() {
    let out = collect(vec![Ok(1), Ok(2), Ok(3), Ok(4)], |item| {
        Ok(item.is_multiple_of(2))
    })
    .await;
    let values: Vec<_> = out[..2]
        .iter()
        .map(|r| r.as_ref().unwrap().clone())
        .collect();
    assert_eq!(values, [serde_json::json!(2), serde_json::json!(4)]);
    // A feed that simply stops lost its backplane: one terminal error.
    assert_eq!(out.len(), 3);
    assert!(unavailable("subscription interrupted")(&out[2]));
}

#[tokio::test]
async fn an_error_item_is_terminal() {
    let lagged = Err(CratestackError::Unavailable(
        "subscription lagged".to_owned(),
    ));
    let out = collect(vec![Ok(1), lagged, Ok(2)], |_| Ok(true)).await;
    assert_eq!(out.len(), 2);
    assert!(unavailable("subscription lagged")(&out[1]));
}

#[tokio::test]
async fn an_undecided_policy_is_the_terminal_error() {
    let out = collect(vec![Ok(1), Ok(2), Ok(3)], |item| {
        if *item == 2 {
            Err(CratestackError::Unavailable("db down".to_owned()))
        } else {
            Ok(true)
        }
    })
    .await;
    // Item 1, then the error; never item 3 with item 2 silently skipped.
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].as_ref().unwrap(), &serde_json::json!(1));
    assert!(unavailable("db down")(&out[1]));
}

#[tokio::test]
async fn backplane_messages_decode_or_end_the_feed() {
    let envelope = serde_json::json!({
        "event_id": "00000000-0000-0000-0000-000000000000",
        "model": "Widget",
        "operation": "Created",
        "occurred_at": "2026-11-01T00:00:00Z",
        "data": 7,
    });
    let receiver: BackplaneReceiver = Box::pin(stream::iter([
        BackplaneMessage::Payload(Arc::from(envelope.to_string())),
        BackplaneMessage::Lagged,
        BackplaneMessage::Resync,
    ]));
    let decoded: Vec<_> = model_events::<u32>(receiver).collect().await;
    assert_eq!(decoded[0].as_ref().unwrap().data, 7);
    assert!(
        matches!(&decoded[1], Err(CratestackError::Unavailable(m)) if m == "subscription lagged")
    );
    assert!(
        matches!(&decoded[2], Err(CratestackError::Unavailable(m)) if m == "subscription interrupted")
    );
}
