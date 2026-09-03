//! Distinct-bucket accounting per scope: the in-memory half of
//! cratestack#871's cardinality budget.
//!
//! One entry per scope (a verified peer, or the process-global scope),
//! holding the set of bucket keys that scope has been allowed to create in
//! the current fixed window. `admit` is the whole contract: "may this
//! scope charge the bucket it asked for, or must it take the fallback?"

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use cratestack_core::BucketBudget;

#[derive(Debug)]
struct Scope {
    members: HashSet<String>,
    /// Start of the fixed window. The window is reset wholesale rather
    /// than expiring members individually: per-member expiry is a sliding
    /// window, which needs an ordered structure and a trim on every
    /// request. See `BucketBudget::window` for why the 2N boundary case
    /// this admits is acceptable.
    opened: Instant,
}

impl Scope {
    fn new(now: Instant) -> Self {
        Self {
            members: HashSet::new(),
            opened: now,
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
    /// keeps its own bucket for the rest of the window even while the
    /// scope is saturated. That is what preserves cratestack#416 under
    /// attack — an attacker filling a peer's budget cannot displace the
    /// legitimate callers already in it.
    pub(super) fn admit(&mut self, budget: &BucketBudget, member: &str, now: Instant) -> bool {
        let scope = self
            .map
            .entry(budget.scope_key.clone())
            .or_insert_with(|| Scope::new(now));
        if now.saturating_duration_since(scope.opened) >= budget.window {
            *scope = Scope::new(now);
        }
        if scope.members.contains(member) {
            return true;
        }
        if scope.members.len() < budget.max_distinct as usize {
            scope.members.insert(member.to_owned());
            return true;
        }
        false
    }

    /// Drop scopes whose window closed at least `max_window` ago.
    ///
    /// Takes the horizon as an argument rather than remembering each
    /// scope's own window because the sweep runs from the bucket side,
    /// which has no budget in hand — and a scope whose window is longer
    /// than the horizon simply gets re-created on its next request, which
    /// costs one admitted member, not correctness.
    pub(super) fn sweep(&mut self, now: Instant, max_window: Duration) {
        self.map
            .retain(|_, scope| now.saturating_duration_since(scope.opened) < max_window);
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.map.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

        assert!(scopes.admit(&budget, "a", now));
        assert!(scopes.admit(&budget, "b", now));
        assert!(!scopes.admit(&budget, "c", now));
        // An already-admitted member stays admitted even while saturated:
        // an attacker filling a scope must not displace the callers
        // already in it (cratestack#416).
        assert!(scopes.admit(&budget, "a", now));
    }

    #[test]
    fn the_window_resets_wholesale() {
        let mut scopes = Scopes::default();
        let budget = budget(1, 60);
        let now = Instant::now();

        assert!(scopes.admit(&budget, "a", now));
        assert!(!scopes.admit(&budget, "b", now));
        assert!(scopes.admit(&budget, "b", now + Duration::from_secs(60)));
    }

    #[test]
    fn sweep_drops_scopes_whose_window_has_closed() {
        let mut scopes = Scopes::default();
        let budget = budget(4, 60);
        let now = Instant::now();
        scopes.admit(&budget, "a", now);
        assert_eq!(scopes.len(), 1);

        scopes.sweep(now + Duration::from_secs(30), Duration::from_secs(60));
        assert_eq!(scopes.len(), 1, "a live window must survive the sweep");

        scopes.sweep(now + Duration::from_secs(61), Duration::from_secs(60));
        assert_eq!(scopes.len(), 0);
    }
}
