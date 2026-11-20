use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::*;

fn session(name: &str) -> RevocationKey {
    RevocationKey::Session(name.to_owned())
}

#[tokio::test]
async fn revoke_ends_a_matching_watch_and_not_others() {
    let revocations = StreamRevocations::default();
    let mut mine = revocations.watch(vec![session("a")]).unwrap();
    let other = revocations.watch(vec![session("b")]).unwrap();

    assert_eq!(
        revocations.revoke(session("a"), StreamEndReason::SessionEnded, None),
        1
    );

    let end = mine.ended().await;
    assert_eq!(end.reason, StreamEndReason::SessionEnded);
    assert_eq!(end.message, "session ended");
    assert!(other.try_ended().is_none());
}

#[tokio::test]
async fn a_watch_under_several_keys_ends_once_and_keeps_the_first_reason() {
    let revocations = StreamRevocations::default();
    let mut watch = revocations
        .watch(vec![RevocationKey::Subject("u".into()), session("s")])
        .unwrap();

    assert_eq!(
        revocations.revoke(session("s"), StreamEndReason::SessionEnded, None),
        1
    );
    // The stream already knows why it ended: a second revoke counts nothing.
    assert_eq!(
        revocations.revoke(
            RevocationKey::Subject("u".into()),
            StreamEndReason::PrincipalSuspended,
            None
        ),
        0
    );
    assert_eq!(watch.ended().await.reason, StreamEndReason::SessionEnded);
}

#[test]
fn dropping_a_watch_deregisters_it() {
    let revocations = StreamRevocations::default();
    let watch = revocations.watch(vec![session("a")]).unwrap();
    assert_eq!(revocations.watching(), 1);
    drop(watch);
    assert_eq!(revocations.watching(), 0);
    assert_eq!(
        revocations.revoke(session("a"), StreamEndReason::SessionEnded, None),
        0
    );
}

static NOW_SECS: AtomicU64 = AtomicU64::new(1_000);

fn fake_now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(NOW_SECS.load(Ordering::SeqCst))
}

fn at(secs: u64) -> Option<SystemTime> {
    Some(UNIX_EPOCH + Duration::from_secs(secs))
}

// One test owns the fake clock, so the others never see it move.
#[test]
fn a_tombstone_refuses_a_later_watch_until_it_lapses() {
    NOW_SECS.store(1_000, Ordering::SeqCst);
    let revocations = StreamRevocations::default().with_clock(fake_now);

    revocations.revoke(session("a"), StreamEndReason::DeviceRevoked, at(1_100));
    let refused = revocations.watch(vec![session("a")]).unwrap_err();
    assert_eq!(refused.reason, StreamEndReason::DeviceRevoked);
    assert!(revocations.watch(vec![session("b")]).is_ok());

    NOW_SECS.store(1_100, Ordering::SeqCst);
    assert!(revocations.watch(vec![session("a")]).is_ok());
    assert_eq!(revocations.tombstones(), 0);
}

#[test]
fn no_until_keeps_no_tombstone() {
    let revocations = StreamRevocations::default();
    revocations.revoke(session("a"), StreamEndReason::SessionEnded, None);
    assert!(revocations.watch(vec![session("a")]).is_ok());
    assert_eq!(revocations.tombstones(), 0);
}

#[test]
fn an_already_lapsed_until_keeps_no_tombstone() {
    let revocations = StreamRevocations::default();
    revocations.revoke(session("a"), StreamEndReason::SessionEnded, at(1));
    assert_eq!(revocations.tombstones(), 0);
}

#[test]
fn a_revoke_flood_cannot_grow_the_tombstones_past_the_cap() {
    let far = SystemTime::now() + Duration::from_secs(3_600);
    let revocations = StreamRevocations::default().with_max_tombstones(3);
    for index in 0..50 {
        let until = far + Duration::from_secs(index);
        revocations.revoke(
            session(&format!("s{index}")),
            StreamEndReason::SessionEnded,
            Some(until),
        );
    }
    assert_eq!(revocations.tombstones(), 3);
    // The latest-lapsing ones survive; the soonest were dropped first.
    assert!(revocations.watch(vec![session("s49")]).is_err());
    assert!(revocations.watch(vec![session("s0")]).is_ok());
}

#[test]
fn extending_a_tombstone_moves_it_in_the_eviction_order() {
    let far = SystemTime::now() + Duration::from_secs(3_600);
    let later = |secs| Some(far + Duration::from_secs(secs));
    let revocations = StreamRevocations::default().with_max_tombstones(2);
    revocations.revoke(session("a"), StreamEndReason::SessionEnded, later(1));
    revocations.revoke(session("b"), StreamEndReason::SessionEnded, later(2));
    // `a` now lapses after `b`, so `c` evicts `b`, not `a`.
    revocations.revoke(session("a"), StreamEndReason::SessionEnded, later(3));
    revocations.revoke(session("c"), StreamEndReason::SessionEnded, later(4));
    assert_eq!(revocations.tombstones(), 2);
    assert!(revocations.watch(vec![session("a")]).is_err());
    assert!(revocations.watch(vec![session("c")]).is_err());
    assert!(revocations.watch(vec![session("b")]).is_ok());
}

#[test]
fn a_lapsed_tombstone_is_not_refused_before_it_is_pruned() {
    // Its own clock: `NOW_SECS` belongs to the test above.
    static PRUNE_NOW: AtomicU64 = AtomicU64::new(5_000);
    fn now() -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(PRUNE_NOW.load(Ordering::SeqCst))
    }
    let revocations = StreamRevocations::default().with_clock(now);
    revocations.revoke(session("a"), StreamEndReason::DeviceRevoked, at(5_100));
    PRUNE_NOW.store(5_200, Ordering::SeqCst);
    // Opening reads the clock but prunes nothing: the entry is still held.
    assert!(revocations.watch(vec![session("a")]).is_ok());
    assert_eq!(revocations.state.lock().unwrap().tombstones.len(), 1);
    assert_eq!(revocations.tombstones(), 0);
}

#[test]
fn a_poisoned_lock_is_recovered() {
    let revocations = StreamRevocations::default();
    let clone = revocations.clone();
    let _ = std::thread::spawn(move || {
        let _guard = clone.state.lock().unwrap();
        panic!("poison the registry");
    })
    .join();
    assert!(revocations.state.is_poisoned());

    let watch = revocations.watch(vec![session("a")]).unwrap();
    assert_eq!(
        revocations.revoke(session("a"), StreamEndReason::SessionEnded, None),
        1
    );
    assert!(watch.try_ended().is_some());
}

#[test]
fn a_resubscribe_reason_keeps_no_tombstone() {
    let revocations = StreamRevocations::default();
    let watch = revocations.watch(vec![session("a")]).unwrap();
    let until = Some(SystemTime::now() + Duration::from_secs(3600));
    assert_eq!(
        revocations.revoke(session("a"), StreamEndReason::StreamLifetime, until),
        1
    );
    assert_eq!(
        watch.try_ended().unwrap().reason,
        StreamEndReason::StreamLifetime
    );
    // A tombstone would refuse every retry with the reason that tells the
    // client to retry: an endless reconnect loop.
    assert!(revocations.watch(vec![session("a")]).is_ok());
    revocations.revoke(session("b"), StreamEndReason::RevocationResync, until);
    assert!(revocations.watch(vec![session("b")]).is_ok());
    assert_eq!(revocations.tombstones(), 0);
}
