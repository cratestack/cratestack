use std::time::Duration;

use cratestack_core::{CratestackError, SubscriptionStream};
use futures_util::StreamExt;
use futures_util::stream;
use tokio::time::Instant;

use super::{SignedStreamDeadline, until_stream_deadline};

fn forever() -> SubscriptionStream<serde_json::Value> {
    Box::pin(stream::pending())
}

fn in_secs(seconds: u64) -> Option<SignedStreamDeadline> {
    Some(SignedStreamDeadline(
        Instant::now() + Duration::from_secs(seconds),
    ))
}

#[tokio::test(start_paused = true)]
async fn a_live_stream_ends_unavailable_at_the_deadline() {
    let mut items = until_stream_deadline(forever(), in_secs(30));
    match items.next().await {
        Some(Err(CratestackError::Unavailable(message))) => assert_eq!(message, "stream lifetime"),
        other => panic!("expected the lifetime error, got {other:?}"),
    }
    assert!(items.next().await.is_none(), "nothing after the end");
}

#[tokio::test(start_paused = true)]
async fn items_before_the_deadline_pass_and_none_means_no_deadline() {
    let source = || -> SubscriptionStream<serde_json::Value> {
        Box::pin(stream::iter([Ok(serde_json::json!(1))]))
    };
    let out: Vec<_> = until_stream_deadline(source(), None).collect().await;
    assert_eq!(out.len(), 1);
    let out: Vec<_> = until_stream_deadline(source(), in_secs(3600))
        .collect()
        .await;
    assert_eq!(
        out.len(),
        1,
        "a stream that ends first ends without the error"
    );
}

/// A source that is always ready must not starve the deadline: a passed
/// deadline ends the stream before another item is yielded.
#[tokio::test(start_paused = true)]
async fn a_busy_source_cannot_starve_the_deadline() {
    let busy: SubscriptionStream<serde_json::Value> =
        Box::pin(stream::repeat_with(|| Ok(serde_json::json!(1))));
    let passed = Some(SignedStreamDeadline(Instant::now()));
    let mut items = until_stream_deadline(busy, passed);
    match items.next().await {
        Some(Err(CratestackError::Unavailable(message))) => assert_eq!(message, "stream lifetime"),
        other => panic!("expected the lifetime error first, got {other:?}"),
    }
    assert!(items.next().await.is_none(), "nothing after the end");
}
