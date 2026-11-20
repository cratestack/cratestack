use std::sync::Arc;
use std::time::Duration;

use super::*;
use crate::backplane::{InProcessBackplane, SubscriptionBackplane};

fn key() -> RevocationKey {
    RevocationKey::Session("s".to_owned())
}

/// Publishes until the follower (which subscribes on its first poll) has
/// applied it, or fails after a second.
async fn publish_until_ended(
    publisher: &StreamRevocations,
    backplane: &InProcessBackplane,
    watch: &mut RevocationWatch,
) -> crate::StreamEndNotice {
    for _ in 0..100 {
        publisher
            .publish(backplane, key(), StreamEndReason::SessionEnded, None)
            .await
            .unwrap();
        if let Ok(end) = tokio::time::timeout(Duration::from_millis(10), watch.ended()).await {
            return end;
        }
    }
    panic!("the revocation never reached the follower");
}

#[tokio::test]
async fn a_revocation_published_on_one_registry_ends_a_stream_on_another() {
    let backplane = Arc::new(InProcessBackplane::default());
    let replica_a = StreamRevocations::default();
    let replica_b = StreamRevocations::default();
    let mut watch = replica_b.watch(vec![key()]).unwrap();
    let follower = tokio::spawn(replica_b.follow(backplane.clone()));

    let end = publish_until_ended(&replica_a, &backplane, &mut watch).await;
    assert_eq!(end.reason, StreamEndReason::SessionEnded);
    follower.abort();
}

#[tokio::test]
async fn a_published_tombstone_reaches_the_other_replica() {
    let backplane = Arc::new(InProcessBackplane::default());
    let (replica_a, replica_b) = (StreamRevocations::default(), StreamRevocations::default());
    let mut probe = replica_b.watch(vec![key()]).unwrap();
    let follower = tokio::spawn(replica_b.follow(backplane.clone()));
    let until = std::time::SystemTime::now() + Duration::from_secs(60);
    for _ in 0..100 {
        replica_a
            .publish(
                &*backplane,
                key(),
                StreamEndReason::DeviceRevoked,
                Some(until),
            )
            .await
            .unwrap();
        if tokio::time::timeout(Duration::from_millis(10), probe.ended())
            .await
            .is_ok()
        {
            break;
        }
    }
    assert_eq!(
        replica_b.watch(vec![key()]).unwrap_err().reason,
        StreamEndReason::DeviceRevoked
    );
    follower.abort();
}

#[tokio::test]
async fn a_malformed_payload_is_skipped() {
    let backplane = Arc::new(InProcessBackplane::default());
    let registry = StreamRevocations::default();
    let watch = registry.watch(vec![key()]).unwrap();
    let follower = tokio::spawn(registry.follow(backplane.clone()));
    for _ in 0..20 {
        backplane
            .publish(REVOCATION_CHANNEL, "{nope")
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(watch.try_ended().is_none());
    assert!(
        !follower.is_finished(),
        "the follower must outlive a bad payload"
    );
    follower.abort();
}

#[tokio::test]
async fn a_byte_message_is_skipped_and_the_follower_keeps_running() {
    let backplane = Arc::new(InProcessBackplane::default());
    let (replica_a, replica_b) = (StreamRevocations::default(), StreamRevocations::default());
    let mut watch = replica_b.watch(vec![key()]).unwrap();
    let follower = tokio::spawn(replica_b.follow(backplane.clone()));
    for _ in 0..20 {
        backplane
            .publish_bytes(REVOCATION_CHANNEL, b"\xa1\x01\x02")
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(watch.try_ended().is_none(), "bytes revoked nothing");
    assert!(!follower.is_finished());
    // Still listening: a real revocation after the stray bytes lands.
    let end = publish_until_ended(&replica_a, &backplane, &mut watch).await;
    assert_eq!(end.reason, StreamEndReason::SessionEnded);
    follower.abort();
}

/// Publishes `payload` until the follower has applied a revoke of `key()`.
async fn publish_raw_until_ended(
    backplane: &InProcessBackplane,
    payload: &str,
    watch: &mut RevocationWatch,
) -> Option<crate::StreamEndNotice> {
    for _ in 0..100 {
        backplane
            .publish(REVOCATION_CHANNEL, payload)
            .await
            .unwrap();
        if let Ok(end) = tokio::time::timeout(Duration::from_millis(10), watch.ended()).await {
            return Some(end);
        }
    }
    None
}

#[tokio::test]
async fn an_overflowing_until_still_revokes_and_keeps_the_follower_alive() {
    let backplane = Arc::new(InProcessBackplane::default());
    let registry = StreamRevocations::default();
    let mut watch = registry.watch(vec![key()]).unwrap();
    let follower = tokio::spawn(registry.follow(backplane.clone()));
    let hostile =
        r#"{"kind":"session","key":"s","reason":"session_ended","until":18446744073709551615}"#;
    let end = publish_raw_until_ended(&backplane, hostile, &mut watch).await;
    assert_eq!(
        end.expect("the key must still be revoked").reason,
        StreamEndReason::SessionEnded
    );
    assert!(
        !follower.is_finished(),
        "the follower must survive the payload"
    );
    // The key is tombstoned (clamped), and a later revoke of another key lands.
    assert!(registry.watch(vec![key()]).is_err());
    let other = RevocationKey::Session("t".to_owned());
    let mut other_watch = registry.watch(vec![other]).unwrap();
    let later = r#"{"kind":"session","key":"t","reason":"device_revoked","until":null}"#;
    let end = publish_raw_until_ended(&backplane, later, &mut other_watch).await;
    assert_eq!(
        end.expect("a later revoke must land").reason,
        StreamEndReason::DeviceRevoked
    );
    follower.abort();
}

#[tokio::test]
async fn an_unknown_reason_still_revokes_as_session_ended() {
    let backplane = Arc::new(InProcessBackplane::default());
    let registry = StreamRevocations::default();
    let mut watch = registry.watch(vec![key()]).unwrap();
    let follower = tokio::spawn(registry.follow(backplane.clone()));
    let newer = r#"{"kind":"session","key":"s","reason":"quarantined","until":null}"#;
    let end = publish_raw_until_ended(&backplane, newer, &mut watch).await;
    assert_eq!(
        end.expect("an unknown reason must still revoke").reason,
        StreamEndReason::SessionEnded
    );
    follower.abort();
}

#[test]
fn resync_policy_defaults_to_failing_closed() {
    assert_eq!(ResyncPolicy::default(), ResyncPolicy::EndAll);
}

#[test]
fn resync_ends_every_stream_unless_ignored() {
    let registry = StreamRevocations::default();
    let watch = registry.watch(vec![key()]).unwrap();
    registry.resync(ResyncPolicy::Ignore);
    assert!(watch.try_ended().is_none());
    registry.resync(ResyncPolicy::EndAll);
    assert_eq!(
        watch.try_ended().unwrap().reason,
        StreamEndReason::RevocationResync
    );
}
