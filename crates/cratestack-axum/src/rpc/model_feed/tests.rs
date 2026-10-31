use cratestack_core::CratestackError;
use futures_util::StreamExt;
use futures_util::stream;

use super::model_feed;

async fn collect(
    items: Vec<u32>,
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

#[tokio::test]
async fn readable_events_pass_and_the_rest_are_dropped() {
    let out = collect(vec![1, 2, 3, 4], |item| Ok(item.is_multiple_of(2))).await;
    let values: Vec<_> = out[..2]
        .iter()
        .map(|r| r.as_ref().unwrap().clone())
        .collect();
    assert_eq!(values, [serde_json::json!(2), serde_json::json!(4)]);
    // The receiver ending means the channel overflowed: one terminal error.
    assert_eq!(out.len(), 3);
    match &out[2] {
        Err(CratestackError::Unavailable(message)) => assert_eq!(message, "subscription lagged"),
        other => panic!("expected the lagged error, got {other:?}"),
    }
}

#[tokio::test]
async fn an_undecided_policy_is_the_terminal_error() {
    let out = collect(vec![1, 2, 3], |item| {
        if *item == 2 {
            Err(CratestackError::Unavailable("db down".to_owned()))
        } else {
            Ok(true)
        }
    })
    .await;
    // Item 1, then the error; never item 3 with item 2 silently skipped,
    // and no lagged error after it.
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].as_ref().unwrap(), &serde_json::json!(1));
    match &out[1] {
        Err(CratestackError::Unavailable(message)) => assert_eq!(message, "db down"),
        other => panic!("expected the policy error, got {other:?}"),
    }
}
