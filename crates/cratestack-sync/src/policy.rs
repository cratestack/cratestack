//! TTLs and protection limits, shared by server and client.
//!
//! The three TTLs are not independent. [`SyncPolicy::validate`] enforces the
//! one ordering that protects correctness rather than just storage:
//!
//! - `client_ttl > mutation_ttl`. A client unseen for `client_ttl` is evicted
//!   and its `last_mutation_seq` forgotten. If it comes back it re-registers
//!   under a new id and re-pushes its pending mutations from seq 1 — so any
//!   mutation the old registration had already applied would apply twice.
//!   Requiring every such mutation to be older than `mutation_ttl` (and so
//!   rejected as expired) closes that window.
//!
//! `tombstone_ttl >= client_ttl` is recommended, not required: a smaller
//! value is still correct, it just sends live clients through a full resync
//! (`cursor_expired`) more often than necessary.

use chrono::TimeDelta;
use serde::{Deserialize, Serialize};

/// Server and client limits. Construct with [`SyncPolicy::default`] and
/// override fields, then call [`SyncPolicy::validate`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SyncPolicy {
    /// A mutation whose `created_at` is older than this when it reaches the
    /// server is rejected as `expired` instead of applied. Stale offline
    /// intent should not silently overwrite a week of other people's work.
    pub mutation_ttl_secs: i64,
    /// A registered client unseen (no push, pull or poke) for this long is
    /// evicted by GC; its next request gets `client_expired`.
    pub client_ttl_secs: i64,
    /// Tombstones (deletes, and rows leaving a scope) older than this are
    /// purged. A cursor older than the purge horizon gets `cursor_expired`.
    pub tombstone_ttl_secs: i64,
    /// Tolerated client clock skew when checking `created_at` in the future.
    pub max_clock_skew_secs: i64,
    /// Upper bound on mutations accepted in one push.
    pub max_mutations_per_push: usize,
    /// Upper bound on one mutation's serialized `args`.
    pub max_mutation_bytes: usize,
    /// Upper bound on a request body, enforced before decoding.
    pub max_body_bytes: usize,
    /// Upper bound on changes returned by one pull (one transaction is never
    /// split, so a page can exceed this by the size of its last transaction).
    pub max_pull_changes: u32,
    /// Registered clients one principal may hold at once.
    pub max_clients_per_principal: i64,
    /// Concurrent poke streams one principal may hold open.
    pub max_poke_streams_per_principal: usize,
    /// Pending mutations a client may queue while offline.
    pub max_pending_mutations: u64,
}

impl Default for SyncPolicy {
    fn default() -> Self {
        const DAY: i64 = 24 * 60 * 60;
        Self {
            mutation_ttl_secs: 7 * DAY,
            client_ttl_secs: 30 * DAY,
            tombstone_ttl_secs: 30 * DAY,
            max_clock_skew_secs: 5 * 60,
            max_mutations_per_push: 100,
            max_mutation_bytes: 16 * 1024,
            max_body_bytes: 1024 * 1024,
            max_pull_changes: 500,
            max_clients_per_principal: 20,
            max_poke_streams_per_principal: 8,
            max_pending_mutations: 10_000,
        }
    }
}

/// A [`SyncPolicy`] that would make the engine incorrect, not merely slow.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PolicyError {
    #[error("{0} must be positive")]
    NotPositive(&'static str),
    #[error(
        "client_ttl ({client}s) must exceed mutation_ttl ({mutation}s): an evicted client re-pushes from seq 1, so a shorter client TTL lets an already-applied mutation apply twice"
    )]
    ClientTtlTooShort { client: i64, mutation: i64 },
}

impl SyncPolicy {
    pub fn validate(&self) -> Result<(), PolicyError> {
        let positive = [
            ("mutation_ttl_secs", self.mutation_ttl_secs),
            ("client_ttl_secs", self.client_ttl_secs),
            ("tombstone_ttl_secs", self.tombstone_ttl_secs),
            ("max_mutations_per_push", self.max_mutations_per_push as i64),
            ("max_mutation_bytes", self.max_mutation_bytes as i64),
            ("max_body_bytes", self.max_body_bytes as i64),
            ("max_pull_changes", i64::from(self.max_pull_changes)),
            ("max_clients_per_principal", self.max_clients_per_principal),
        ];
        if let Some((name, _)) = positive.iter().find(|(_, value)| *value <= 0) {
            return Err(PolicyError::NotPositive(name));
        }
        if self.client_ttl_secs <= self.mutation_ttl_secs {
            return Err(PolicyError::ClientTtlTooShort {
                client: self.client_ttl_secs,
                mutation: self.mutation_ttl_secs,
            });
        }
        Ok(())
    }

    pub fn mutation_ttl(&self) -> TimeDelta {
        TimeDelta::seconds(self.mutation_ttl_secs)
    }

    pub fn client_ttl(&self) -> TimeDelta {
        TimeDelta::seconds(self.client_ttl_secs)
    }

    pub fn tombstone_ttl(&self) -> TimeDelta {
        TimeDelta::seconds(self.tombstone_ttl_secs)
    }

    pub fn max_clock_skew(&self) -> TimeDelta {
        TimeDelta::seconds(self.max_clock_skew_secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_is_valid() {
        SyncPolicy::default().validate().unwrap();
    }

    #[test]
    fn rejects_client_ttl_not_exceeding_mutation_ttl() {
        let policy = SyncPolicy {
            client_ttl_secs: 60,
            mutation_ttl_secs: 60,
            ..SyncPolicy::default()
        };
        assert!(matches!(
            policy.validate(),
            Err(PolicyError::ClientTtlTooShort { .. })
        ));
    }

    #[test]
    fn rejects_zero_limits() {
        let policy = SyncPolicy {
            max_pull_changes: 0,
            ..SyncPolicy::default()
        };
        assert_eq!(
            policy.validate(),
            Err(PolicyError::NotPositive("max_pull_changes"))
        );
    }
}
