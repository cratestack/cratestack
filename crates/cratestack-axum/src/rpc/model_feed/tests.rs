use std::sync::Arc;

use cratestack_codec_json::JsonCodec;
use cratestack_core::rpc::RpcFrameValue;
use cratestack_core::{BackplaneMessage, BackplaneReceiver, CratestackError, ModelEventKind};
use futures_util::StreamExt;
use futures_util::stream;

use super::{FeedEvent, model_events, model_feed, route_event};

async fn collect(
    items: Vec<Result<u32, CratestackError>>,
    readable: impl FnMut(&u32) -> Result<bool, CratestackError> + Send + 'static,
) -> Vec<Result<RpcFrameValue, CratestackError>> {
    let mut readable = readable;
    model_feed(stream::iter(items), JsonCodec, move |item: u32| {
        let verdict = readable(&item).map(|readable| readable.then_some(item));
        async move { verdict }
    })
    .collect()
    .await
}

fn unavailable(message: &str) -> impl Fn(&Result<RpcFrameValue, CratestackError>) -> bool + '_ {
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
    assert_eq!(
        values,
        [
            RpcFrameValue::from(serde_json::json!(2)),
            RpcFrameValue::from(serde_json::json!(4))
        ]
    );
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
    assert_eq!(
        out[0].as_ref().unwrap(),
        &RpcFrameValue::from(serde_json::json!(1))
    );
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
    assert_eq!(decoded[0].as_ref().unwrap().event.data, 7);
    assert!(
        matches!(&decoded[1], Err(CratestackError::Unavailable(m)) if m == "subscription lagged")
    );
    assert!(
        matches!(&decoded[2], Err(CratestackError::Unavailable(m)) if m == "subscription interrupted")
    );
}

/// An event as it comes off the wire, `matches` absent: it defaults to `true`.
fn feed(operation: ModelEventKind, data: u32, before: Option<u32>) -> FeedEvent<u32> {
    let event = serde_json::from_value(serde_json::json!({
        "event_id": "00000000-0000-0000-0000-000000000000",
        "model": "Widget",
        "operation": operation,
        "occurred_at": "2026-11-06T00:00:00Z",
        "data": data,
    }))
    .unwrap();
    FeedEvent { event, before }
}

/// A subscriber filtering on `< 10` that may read only even rows.
async fn route(feed: FeedEvent<u32>) -> Option<(bool, u32)> {
    route_event(
        feed,
        true,
        |n| *n < 10,
        |n| {
            let readable = n % 2 == 0;
            async move { Ok(readable) }
        },
    )
    .await
    .unwrap()
    .map(|event| (event.matches, event.data))
}

#[tokio::test]
async fn an_event_in_the_filter_is_delivered_as_is() {
    assert_eq!(
        route(feed(ModelEventKind::Created, 4, None)).await,
        Some((true, 4))
    );
    // The pre-image is not consulted when the post-image passes.
    assert_eq!(
        route(feed(ModelEventKind::Updated, 4, Some(40))).await,
        Some((true, 4))
    );
}

#[tokio::test]
async fn an_update_out_of_the_filter_delivers_the_pre_image_only() {
    // 4 matched; 12 (the post-image) does not: the subscriber gets 4, not 12.
    assert_eq!(
        route(feed(ModelEventKind::Updated, 12, Some(4))).await,
        Some((false, 4))
    );
}

#[tokio::test]
async fn an_update_out_of_the_read_policy_delivers_the_pre_image_only() {
    // 5 is in the filter but unreadable: the row left the policy, and 4 is
    // what the subscriber held.
    assert_eq!(
        route(feed(ModelEventKind::Updated, 5, Some(4))).await,
        Some((false, 4))
    );
}

#[tokio::test]
async fn an_update_the_subscriber_never_saw_is_dropped() {
    // Neither image in the filter.
    assert_eq!(
        route(feed(ModelEventKind::Updated, 14, Some(12))).await,
        None
    );
    // The pre-image was in the filter but unreadable: nothing was held.
    assert_eq!(
        route(feed(ModelEventKind::Updated, 14, Some(5))).await,
        None
    );
    // No pre-image (a bulk update): no exit event.
    assert_eq!(route(feed(ModelEventKind::Updated, 12, None)).await, None);
}

#[tokio::test]
async fn an_unwanted_operation_is_dropped_whatever_the_images() {
    let dropped = route_event(
        feed(ModelEventKind::Updated, 12, Some(4)),
        false,
        |_| true,
        |_| async { Ok(true) },
    )
    .await
    .unwrap();
    assert!(dropped.is_none());
}
