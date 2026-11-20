use std::time::{SystemTime, UNIX_EPOCH};

use cratestack_core::{CratestackError, SubscriptionStream};
use futures_util::StreamExt;
use futures_util::stream;

use super::until_credential_expires;

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

fn forever() -> SubscriptionStream<serde_json::Value> {
    Box::pin(stream::pending())
}

#[tokio::test(start_paused = true)]
async fn a_live_stream_ends_when_the_credential_expires() {
    let mut items = until_credential_expires(forever(), Some(now() + 30));
    match items.next().await {
        Some(Err(CratestackError::StreamEnded(end))) => {
            assert_eq!(end.message, "credential expired")
        }
        other => panic!("expected the expiry error, got {other:?}"),
    }
    assert!(items.next().await.is_none(), "nothing after the expiry");
}

#[tokio::test]
async fn an_already_expired_credential_ends_at_once() {
    let mut items = until_credential_expires(forever(), Some(now() - 1));
    assert!(matches!(
        items.next().await,
        Some(Err(CratestackError::StreamEnded(_)))
    ));
}

#[tokio::test]
async fn items_pass_through_and_no_exp_means_no_deadline() {
    let source: SubscriptionStream<serde_json::Value> =
        Box::pin(stream::iter([Ok(serde_json::json!(1))]));
    let out: Vec<_> = until_credential_expires(source, None).collect().await;
    assert_eq!(out.len(), 1);
    let source: SubscriptionStream<serde_json::Value> =
        Box::pin(stream::iter([Ok(serde_json::json!(1))]));
    let out: Vec<_> = until_credential_expires(source, Some(now() + 3600))
        .collect()
        .await;
    assert_eq!(out.len(), 1);
}

/// A source that is always ready must not starve the expiry: an expired
/// credential ends the stream before another item is yielded.
#[tokio::test(start_paused = true)]
async fn a_busy_source_cannot_starve_the_expiry() {
    let busy: SubscriptionStream<serde_json::Value> =
        Box::pin(stream::repeat_with(|| Ok(serde_json::json!(1))));
    let mut items = until_credential_expires(busy, Some(now() - 1));
    match items.next().await {
        Some(Err(CratestackError::StreamEnded(end))) => {
            assert_eq!(end.message, "credential expired")
        }
        other => panic!("expected the expiry error first, got {other:?}"),
    }
    assert!(items.next().await.is_none(), "nothing after the expiry");
}

/// A source that is always ready, yielding to the runtime between items (as a
/// socket-backed body does), is cut shortly after a credential expiry that
/// passes mid-stream: items flow first, then the terminal error, in real time
/// on a runtime whose timer driver runs.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_busy_source_is_cut_when_the_credential_expires_mid_stream() {
    let busy: SubscriptionStream<serde_json::Value> = Box::pin(stream::unfold((), |()| async {
        tokio::task::yield_now().await;
        Some((Ok(serde_json::json!(1)), ()))
    }));
    let started = std::time::Instant::now();
    let mut items = until_credential_expires(busy, Some(now() + 1));
    let mut passed = 0usize;
    let ended = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            match items.next().await {
                Some(Ok(_)) => passed += 1,
                other => break other,
            }
        }
    })
    .await
    .expect("the expiry ended a busy stream");
    match ended {
        Some(Err(CratestackError::StreamEnded(end))) => {
            assert_eq!(end.message, "credential expired")
        }
        other => panic!("expected the expiry error, got {other:?}"),
    }
    assert!(passed > 0, "items flowed before the expiry");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "shortly after"
    );
    assert!(items.next().await.is_none(), "nothing after the end");
}
