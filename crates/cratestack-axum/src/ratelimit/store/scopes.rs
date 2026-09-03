//! Distinct-bucket accounting per scope: the in-memory half of
//! cratestack#871's cardinality budget.
//!
//! One entry per scope (a verified peer, or the process-global scope),
//! holding the bucket keys that scope is allowed to have created, each
//! with its own expiry. `admit` is the whole contract: "may this scope
//! charge the bucket it asked for, or must it take the fallback?"
//!
//! # A sliding window, not a fixed one (cratestack#871 round-2, item 3)
//!
//! Two earlier shapes were wrong in opposite directions. A fixed `window`
//! that reset the whole scope let each new generation admit `max_distinct`
//! more buckets while the previous generation was still alive — 81 buckets
//! over 20 windows for a cap of 4. Extending one shared deadline instead
//! fixed that, but left a transient `2 x max_distinct` when a bucket
//! outlived the record that admitted it.
//!
//! Members now expire **individually**, `scope_ttl` after they were last
//! used, and the scope entry lives as long as any member does. So the
//! record can never expire underneath a live bucket (no generation opens
//! beneath it, and the transient `2N` is gone), and a peer whose
//! credentials rotate is never permanently capped — the slots of tokens it
//! stopped using age out on their own.
//!
//! The stored `Instant` is the member's **expiry, refreshed on every hit**,
//! not its first admission. Scoring by first admission would let an
//! attacker keep a bucket alive by consuming it while its slot aged out
//! underneath, freeing room for another — which is the `2N` again.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use cratestack_core::BucketBudget;

#[derive(Debug, Default)]
struct Scope {
    /// Bucket key -> when this member's slot expires.
    members: HashMap<String, Instant>,
}

impl Scope {
    /// Drop members whose slot has aged out. Bounded by `max_distinct`
    /// (128 by default), and it is the per-request trim that makes the
    /// window slide.
    fn expire(&mut self, now: Instant) {
        self.members.retain(|_, expires_at| now < *expires_at);
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
    /// turns into a charge against the budget's fallback bucket. An
    /// already-known member is admitted *before* the cap is consulted, so
    /// an attacker filling a peer's budget cannot displace the legitimate
    /// callers already in it (cratestack#416).
    ///
    /// `scope_ttl` comes from [`cratestack_core::scope_ttl_secs`] and is
    /// never shorter than the bucket TTL, so a member's slot outlives the
    /// bucket it admitted.
    pub(super) fn admit(
        &mut self,
        budget: &BucketBudget,
        member: &str,
        now: Instant,
        scope_ttl: Duration,
    ) -> bool {
        let expires_at = now.checked_add(scope_ttl).unwrap_or(now);
        let scope = self.map.entry(budget.scope_key.clone()).or_default();
        scope.expire(now);
        if let Some(slot) = scope.members.get_mut(member) {
            // Refresh on use: an actively-used member keeps its slot, so
            // its bucket is always accounted for. One that goes quiet ages
            // out and frees the slot for a rotated credential.
            *slot = expires_at;
            return true;
        }
        if scope.members.len() < budget.max_distinct as usize {
            scope.members.insert(member.to_owned(), expires_at);
            return true;
        }
        false
    }

    /// Whether this scope already has an entry — lets the store tell "this
    /// request needs a NEW scope" from "it reuses one" *before* deciding
    /// whether there is room for it (cratestack#871 round-2, item 2).
    pub(super) fn contains(&self, scope_key: &str) -> bool {
        self.map.contains_key(scope_key)
    }

    pub(super) fn len(&self) -> usize {
        self.map.len()
    }

    /// Drop expired members, and any scope left with none. No horizon
    /// argument: every member carries its own deadline, so there is no way
    /// to pass the wrong one.
    pub(super) fn sweep(&mut self, now: Instant) {
        self.map.retain(|_, scope| {
            scope.expire(now);
            !scope.members.is_empty()
        });
    }
}

#[cfg(test)]
mod tests;
