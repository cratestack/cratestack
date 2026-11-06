use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};

use super::{
    BackplaneMessage, BackplaneReceiver, InProcessBackplane, SUBSCRIPTION_BUFFER_CAPACITY,
    SubscriptionBackplane, TopicHandle,
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
