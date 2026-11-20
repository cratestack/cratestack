use std::time::{SystemTime, UNIX_EPOCH};

use cratestack_core::{
    CratestackError, RevocationKey, StreamEndNotice, StreamEndReason, StreamRevocations,
    SubscriptionStream,
};
use futures_util::StreamExt;
use futures_util::stream;

use super::{StreamBounds, until_principal_ends};

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

fn key() -> RevocationKey {
    RevocationKey::Session("s".to_owned())
}

fn forever() -> SubscriptionStream<serde_json::Value> {
    Box::pin(stream::pending())
}

fn busy() -> SubscriptionStream<serde_json::Value> {
    Box::pin(stream::repeat_with(|| Ok(serde_json::json!(1))))
}

fn reason_of(item: Option<Result<serde_json::Value, CratestackError>>) -> StreamEndReason {
    match item {
        Some(Err(CratestackError::StreamEnded(end))) => end.reason,
        other => panic!("expected a typed stream end, got {other:?}"),
    }
}

fn bounds(
    revocations: &StreamRevocations,
    expires_at: Option<i64>,
) -> (StreamBounds, cratestack_core::RevocationWatch) {
    // A second watch under the same key, to observe the registry.
    let watch = revocations.watch(vec![key()]).unwrap();
    (
        StreamBounds {
            expires_at,
            revocation: Some(revocations.watch(vec![key()]).unwrap()),
        },
        watch,
    )
}

#[tokio::test(start_paused = true)]
async fn a_revoked_principal_ends_a_live_stream_with_its_reason() {
    let revocations = StreamRevocations::default();
    let (bounds, _observer) = bounds(&revocations, None);
    let mut items = until_principal_ends(forever(), bounds);

    let next = tokio::spawn(async move { (items.next().await, items.next().await) });
    tokio::task::yield_now().await;
    revocations.revoke(key(), StreamEndReason::PrincipalSuspended, None);

    let (first, second) = next.await.unwrap();
    assert_eq!(reason_of(first), StreamEndReason::PrincipalSuspended);
    assert!(second.is_none(), "nothing after the end");
}

#[tokio::test]
async fn revocation_beats_an_always_ready_source() {
    let revocations = StreamRevocations::default();
    let (bounds, _observer) = bounds(&revocations, None);
    revocations.revoke(key(), StreamEndReason::SessionEnded, None);
    let mut items = until_principal_ends(busy(), bounds);
    assert_eq!(reason_of(items.next().await), StreamEndReason::SessionEnded);
}

#[tokio::test(start_paused = true)]
async fn revocation_beats_expiry_at_the_same_instant() {
    let revocations = StreamRevocations::default();
    let (bounds, _observer) = bounds(&revocations, Some(now() - 1));
    revocations.revoke(key(), StreamEndReason::DeviceRevoked, None);
    let mut items = until_principal_ends(busy(), bounds);
    assert_eq!(
        reason_of(items.next().await),
        StreamEndReason::DeviceRevoked
    );
}

#[tokio::test(start_paused = true)]
async fn expiry_still_ends_credential_expired_with_a_typed_reason() {
    let revocations = StreamRevocations::default();
    let (bounds, _observer) = bounds(&revocations, Some(now() + 30));
    let mut items = until_principal_ends(forever(), bounds);
    let item = items.next().await;
    let Some(Err(error)) = &item else {
        panic!("expected the expiry, got {item:?}");
    };
    assert_eq!(error.code(), "UNAUTHORIZED");
    assert_eq!(error.public_message(), "credential expired");
    assert_eq!(reason_of(item), StreamEndReason::CredentialExpired);
}

#[tokio::test]
async fn a_source_error_is_still_terminal() {
    let revocations = StreamRevocations::default();
    let (bounds, _observer) = bounds(&revocations, None);
    let source: SubscriptionStream<serde_json::Value> = Box::pin(stream::iter([
        Ok(serde_json::json!(1)),
        Err(CratestackError::Internal("boom".into())),
        Ok(serde_json::json!(2)),
    ]));
    let out: Vec<_> = until_principal_ends(source, bounds).collect().await;
    assert_eq!(out.len(), 2, "nothing after the source error");
    assert!(matches!(out[1], Err(CratestackError::Internal(_))));
}

#[tokio::test]
async fn an_unrevoked_stream_passes_items_through_and_unwatches_when_done() {
    let revocations = StreamRevocations::default();
    let (bounds, observer) = bounds(&revocations, None);
    drop(observer);
    let source: SubscriptionStream<serde_json::Value> =
        Box::pin(stream::iter([Ok(serde_json::json!(1))]));
    let items = until_principal_ends(source, bounds);
    assert_eq!(revocations.watching(), 1);
    let out: Vec<_> = items.collect().await;
    assert_eq!(out.len(), 1);
    assert_eq!(revocations.watching(), 0, "the stream's watch is dropped");
}

#[test]
fn open_refuses_a_tombstoned_principal() {
    use cratestack_core::CratestackContext;

    let revocations = StreamRevocations::default();
    let ctx = CratestackContext::builder("u-1").build();
    revocations.revoke(
        RevocationKey::Subject("u-1".into()),
        StreamEndReason::PrincipalSuspended,
        Some(SystemTime::now() + std::time::Duration::from_secs(60)),
    );
    let refused = StreamBounds::open(&ctx, &revocations).unwrap_err();
    assert_eq!(refused.status_code().as_u16(), 403);
    assert!(matches!(
        refused,
        CratestackError::StreamEnded(StreamEndNotice {
            reason: StreamEndReason::PrincipalSuspended,
            ..
        })
    ));
}
