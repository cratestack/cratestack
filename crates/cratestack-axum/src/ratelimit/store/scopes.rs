//! Distinct-bucket accounting per scope: the in-memory half of
//! cratestack#871's cardinality budget.
//!
//! One entry per scope (a verified peer, or the process-global scope),
//! holding the set of bucket keys that scope has been allowed to create.
//! `admit` is the whole contract: "may this scope charge the bucket it
//! asked for, or must it take the fallback?"
//!
//! # The record must outlive the buckets it admitted
//!
//! The first cut reset the whole scope on a fixed `window`. That bounded
//! nothing, and it was measured (cratestack#871 review, blocker 2): with a
//! window shorter than the bucket TTL, each new generation admitted
//! `max_distinct` more buckets while the previous generation was still
//! alive — 81 buckets over 20 windows for a cap of 4. The lifetime is now
//! [`cratestack_core::scope_ttl_secs`], i.e. at least the buckets' own
//! TTL, and every admission pushes it forward.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use cratestack_core::BucketBudget;

#[derive(Debug)]
struct Scope {
    members: HashSet<String>,
    /// When this admission record dies, taking its whole member set with
    /// it. Pushed forward by every admission — so a scope stays alive
    /// while it is actively admitting, and expires one full lifetime after
    /// it stops. Never shortened.
    expires_at: Instant,
}

impl Scope {
    fn new(expires_at: Instant) -> Self {
        Self {
            members: HashSet::new(),
            expires_at,
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct Scopes {
    map: HashMap<String, Scope>,
}

impl Scopes {
    /// Whether `member` may have its own bucket under `budget`.
    ///
    /// Returns `false` once the scope is at its cap, which the caller
    /// turns into a charge against the budget's fallback bucket. Note the
    /// order: an already-known member is admitted *before* the cap is
    /// consulted, so a caller who was under the cap when it first appeared
    /// keeps its own bucket for the rest of the scope's life even while
    /// the scope is saturated. That is what preserves cratestack#416 under
    /// attack — an attacker filling a peer's budget cannot displace the
    /// legitimate callers already in it.
    ///
    /// `scope_ttl` comes from [`cratestack_core::scope_ttl_secs`] and is
    /// never shorter than the bucket TTL; see the module docs.
    pub(super) fn admit(
        &mut self,
        budget: &BucketBudget,
        member: &str,
        now: Instant,
        scope_ttl: Duration,
    ) -> bool {
        let deadline = now.checked_add(scope_ttl).unwrap_or(now);
        let scope = self
            .map
            .entry(budget.scope_key.clone())
            .or_insert_with(|| Scope::new(deadline));
        if now >= scope.expires_at {
            *scope = Scope::new(deadline);
        }
        if scope.members.contains(member) {
            return true;
        }
        if scope.members.len() < budget.max_distinct as usize {
            scope.members.insert(member.to_owned());
            // Refreshed on admission, not on every hit. A saturated scope
            // therefore ages out and lets its peer start over, instead of
            // capping that peer at its first N credentials forever — which
            // would break any deployment whose tokens rotate.
            scope.expires_at = scope.expires_at.max(deadline);
            return true;
        }
        false
    }

    /// Drop scopes whose lifetime has run out. Each carries its own
    /// deadline, so no horizon has to be passed in — and none can be
    /// passed in wrongly, which is how the previous version leaked.
    pub(super) fn sweep(&mut self, now: Instant) {
        self.map.retain(|_, scope| now < scope.expires_at);
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.map.len()
    }
}

#[cfg(test)]
mod tests {
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

    /// A saturated scope ages out so its peer can start over — otherwise a
    /// deployment whose tokens rotate would be capped at its first N
    /// credentials forever.
    #[test]
    fn a_saturated_scope_expires_and_lets_the_peer_start_over() {
        let mut scopes = Scopes::default();
        let budget = budget(1, 60);
        let now = Instant::now();

        assert!(scopes.admit(&budget, "a", now, TTL));
        assert!(!scopes.admit(&budget, "b", now, TTL));
        assert!(scopes.admit(&budget, "b", now + TTL, TTL));
    }

    /// cratestack#871 review, blocker 2: admissions push the deadline
    /// forward, so a scope that keeps admitting never expires underneath
    /// the buckets it is admitting.
    #[test]
    fn each_admission_extends_the_scope_lifetime() {
        let mut scopes = Scopes::default();
        let budget = budget(10, 60);
        let now = Instant::now();

        assert!(scopes.admit(&budget, "a", now, TTL));
        assert!(scopes.admit(&budget, "b", now + TTL - Duration::from_secs(1), TTL));

        // The ORIGINAL deadline has passed, but the extension means the
        // scope is still live and still remembers `a` — so `a` cannot be
        // re-admitted into a fresh generation, which is how the previous
        // version leaked a second N.
        scopes.sweep(now + TTL + Duration::from_secs(1));
        assert_eq!(scopes.len(), 1, "an extended scope must survive the sweep");
    }

    #[test]
    fn sweep_drops_scopes_whose_lifetime_has_run_out() {
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
}
