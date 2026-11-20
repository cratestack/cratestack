use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;

use crate::error::CratestackError;
use serde::{Deserialize, Serialize};

use super::{
    BackplaneFuture, BackplaneMessage, BackplaneReceiver, InProcessBackplane,
    SUBSCRIPTION_BUFFER_CAPACITY, SubscriptionBackplane, TopicHandle,
};

async fn next(receiver: &mut BackplaneReceiver) -> Option<BackplaneMessage> {
    tokio::time::timeout(Duration::from_secs(1), receiver.next())
        .await
        .expect("a message within 1s")
}

#[tokio::test]
async fn a_subscriber_receives_what_is_published_on_its_channel_only() {
    let backplane = InProcessBackplane::new();
    let mut widgets = backplane.subscribe("w").await.unwrap();
    let mut other = backplane.subscribe("x").await.unwrap();
    backplane.publish("w", "{\"n\":1}").await.unwrap();
    assert_eq!(
        next(&mut widgets).await,
        Some(BackplaneMessage::Payload("{\"n\":1}".into()))
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(50), other.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn a_subscriber_that_falls_behind_is_told_and_ends() {
    let backplane = InProcessBackplane::new();
    let mut slow = backplane.subscribe("w").await.unwrap();
    for n in 0..=SUBSCRIPTION_BUFFER_CAPACITY {
        backplane.publish("w", &n.to_string()).await.unwrap();
    }
    assert_eq!(next(&mut slow).await, Some(BackplaneMessage::Lagged));
    assert_eq!(next(&mut slow).await, None, "nothing after Lagged");
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Message {
    body: String,
}

#[tokio::test]
async fn a_topic_delivers_under_its_key_only() {
    let backplane = Arc::new(InProcessBackplane::new());
    let room = |key: &str| TopicHandle::<Message>::new(backplane.clone(), "RoomMessage", key);
    assert_eq!(room("r1").channel(), "cratestack.topic.RoomMessage.r1");
    let mut r1 = room("r1").subscribe().await.unwrap();
    let mut r2 = room("r2").subscribe().await.unwrap();
    let hello = Message {
        body: "hello".to_owned(),
    };
    room("r1").publish(&hello).await.unwrap();
    assert_eq!(r1.next().await.unwrap().unwrap(), hello);
    let nothing = tokio::time::timeout(Duration::from_millis(50), r2.next()).await;
    assert!(nothing.is_err(), "r2 received r1's message");
}

#[tokio::test]
async fn bytes_arrive_exactly_as_published_mixed_with_text_in_order() {
    let backplane = InProcessBackplane::new();
    let mut receiver = backplane.subscribe("w").await.unwrap();
    let blob: Vec<u8> = (0..=255).collect();
    backplane.publish("w", "one").await.unwrap();
    backplane.publish_bytes("w", &blob).await.unwrap();
    backplane.publish_bytes("w", &[]).await.unwrap();
    backplane.publish("w", "four").await.unwrap();
    assert_eq!(
        next(&mut receiver).await,
        Some(BackplaneMessage::Payload("one".into()))
    );
    assert_eq!(
        next(&mut receiver).await,
        Some(BackplaneMessage::Bytes(Arc::from(blob)))
    );
    assert_eq!(
        next(&mut receiver).await,
        Some(BackplaneMessage::Bytes(Arc::from(Vec::new())))
    );
    assert_eq!(
        next(&mut receiver).await,
        Some(BackplaneMessage::Payload("four".into()))
    );
}

/// A backplane written before byte payloads existed: only the two required
/// methods.
struct TextOnly;

impl SubscriptionBackplane for TextOnly {
    fn publish<'a>(
        &'a self,
        _: &'a str,
        _: &'a str,
    ) -> BackplaneFuture<'a, Result<(), CratestackError>> {
        Box::pin(std::future::ready(Ok(())))
    }

    fn subscribe<'a>(
        &'a self,
        _: &'a str,
    ) -> BackplaneFuture<'a, Result<BackplaneReceiver, CratestackError>> {
        Box::pin(std::future::ready(Ok(
            Box::pin(futures_util::stream::empty()) as BackplaneReceiver,
        )))
    }
}

#[tokio::test]
async fn a_backplane_without_byte_support_refuses_bytes_with_a_typed_error() {
    let error = TextOnly.publish_bytes("w", b"x").await.unwrap_err();
    assert!(
        matches!(&error, CratestackError::Internal(m) if m.contains("does not carry byte payloads")),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_topic_skips_a_stray_byte_message_and_keeps_streaming() {
    let backplane = Arc::new(InProcessBackplane::new());
    let room = TopicHandle::<Message>::new(backplane.clone(), "RoomMessage", "r1");
    let mut stream = room.subscribe().await.unwrap();
    backplane
        .publish_bytes(room.channel(), b"\xff\x00")
        .await
        .unwrap();
    let hello = Message {
        body: "after the bytes".to_owned(),
    };
    room.publish(&hello).await.unwrap();
    let item = tokio::time::timeout(Duration::from_secs(1), stream.next())
        .await
        .expect("an item within 1s");
    assert_eq!(item.unwrap().unwrap(), hello);
}
