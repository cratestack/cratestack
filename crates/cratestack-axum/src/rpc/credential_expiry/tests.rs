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
        Some(Err(CratestackError::Unauthorized(message))) => {
            assert_eq!(message, "credential expired")
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
        Some(Err(CratestackError::Unauthorized(_)))
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
