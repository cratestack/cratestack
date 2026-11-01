use std::time::Duration;

use cratestack_core::{BackplaneMessage, SubscriptionBackplane};
use futures_util::StreamExt;

use super::{InProcessBackplane, SUBSCRIPTION_BUFFER_CAPACITY, notify_payload};

async fn next(receiver: &mut cratestack_core::BackplaneReceiver) -> Option<BackplaneMessage> {
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

#[test]
fn a_large_payload_is_sent_by_reference_when_it_can_be() {
    let small = notify_payload::encode("c", "{}", None).unwrap();
    assert!(small.contains("\"p\""));
    let big = "x".repeat(notify_payload::NOTIFY_LIMIT);
    let id = uuid::Uuid::nil();
    let by_ref = notify_payload::encode("c", &big, Some(id)).unwrap();
    assert!(by_ref.contains("\"r\"") && !by_ref.contains("\"p\""));
    assert!(notify_payload::encode("c", &big, None).is_none());
}
