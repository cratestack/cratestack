//! Unit tests for the per-scope admission index.
//!
//! A sibling file rather than an inline `mod tests`, for the workspace's
//! 200-line ceiling.

use super::*;

const TTL: Duration = Duration::from_secs(60);

fn budget(max_distinct: u32, window_secs: u64) -> BucketBudget {
    BucketBudget::new(
        "peer:192.0.2.1",
        "ip:192.0.2.1",
        max_distinct,
        Duration::from_secs(window_secs),
    )
}

#[test]
fn admits_up_to_the_cap_then_refuses_new_members() {
    let mut scopes = Scopes::default();
    let budget = budget(2, 60);
    let now = Instant::now();

    assert!(scopes.admit(&budget, "a", now, TTL));
    assert!(scopes.admit(&budget, "b", now, TTL));
    assert!(!scopes.admit(&budget, "c", now, TTL));
    // An already-admitted member stays admitted even while saturated:
    // an attacker filling a scope must not displace the callers
    // already in it (cratestack#416).
    assert!(scopes.admit(&budget, "a", now, TTL));
}

/// A member that goes quiet frees its slot, so a peer whose tokens
/// rotate is never permanently capped.
#[test]
fn an_idle_member_ages_out_and_frees_its_slot() {
    let mut scopes = Scopes::default();
    let budget = budget(1, 60);
    let now = Instant::now();

    assert!(scopes.admit(&budget, "a", now, TTL));
    assert!(!scopes.admit(&budget, "b", now, TTL));
    assert!(scopes.admit(&budget, "b", now + TTL, TTL));
}

/// The other half of the slide: a member that keeps being used keeps
/// its slot, so its still-live bucket stays accounted for. Without
/// this, a bucket kept alive by traffic could outlive its slot and let
/// another be admitted alongside it — the transient `2N`.
#[test]
fn an_active_member_keeps_its_slot_indefinitely() {
    let mut scopes = Scopes::default();
    let budget = budget(1, 60);
    let mut now = Instant::now();

    assert!(scopes.admit(&budget, "a", now, TTL));
    for _ in 0..5 {
        now += TTL - Duration::from_secs(1);
        assert!(scopes.admit(&budget, "a", now, TTL), "refreshed on use");
        assert!(
            !scopes.admit(&budget, "b", now, TTL),
            "the slot is still taken, so nothing new may be admitted",
        );
    }
}

/// Members expire one at a time rather than the scope resetting
/// wholesale — which is what stops a fresh generation opening
/// underneath a live one.
#[test]
fn members_expire_individually_not_as_a_generation() {
    let mut scopes = Scopes::default();
    let budget = budget(2, 60);
    let now = Instant::now();

    assert!(scopes.admit(&budget, "old", now, TTL));
    let later = now + Duration::from_secs(30);
    assert!(scopes.admit(&budget, "new", later, TTL));

    // `old` has aged out, `new` has not: exactly one slot is free.
    let after = now + TTL + Duration::from_secs(1);
    assert!(scopes.admit(&budget, "third", after, TTL));
    assert!(
        !scopes.admit(&budget, "fourth", after, TTL),
        "`new` is still holding its slot, so only one was freed",
    );
}

#[test]
fn sweep_drops_scopes_once_every_member_has_expired() {
    let mut scopes = Scopes::default();
    let budget = budget(4, 60);
    let now = Instant::now();
    scopes.admit(&budget, "a", now, TTL);
    assert_eq!(scopes.len(), 1);

    scopes.sweep(now + Duration::from_secs(30));
    assert_eq!(scopes.len(), 1, "a live scope must survive the sweep");

    scopes.sweep(now + Duration::from_secs(61));
    assert_eq!(scopes.len(), 0);
}
