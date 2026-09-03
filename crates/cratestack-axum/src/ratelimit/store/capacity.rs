//! Failing closed at the store's caps, *before* anything is interned.
//!
//! Split from `store.rs` for the workspace's 200-line ceiling when
//! cratestack#871's round-2 review found the ordering bug this module
//! exists to prevent: admitting into the scope index and only then letting
//! the bucket map refuse left a scope entry behind for every refused
//! request.

use std::time::{Duration, Instant};

use cratestack_core::{ConsumeRequest, CratestackError};

use super::{InMemoryRateLimitStore, State};

impl InMemoryRateLimitStore {
    /// Fail closed before the scope index is touched, so a refused
    /// request leaves nothing behind.
    ///
    /// Two caps, both `Internal` (a logical class, refused under every
    /// [`super::StoreErrorPolicy`]): the bucket map, and the scope map.
    /// The scope map needs its own because scope entries outlive the
    /// buckets that created them — `scope_ttl` is at least the bucket TTL
    /// — so bounding buckets alone does not bound them.
    pub(super) fn reserve_admission(
        &self,
        state: &mut State,
        request: &ConsumeRequest<'_>,
        budget: &cratestack_core::BucketBudget,
        now: Instant,
        ttl: Duration,
    ) -> Result<(), CratestackError> {
        // A request whose bucket already exists creates nothing, so it is
        // always servable. Anything else may need one — conservatively,
        // whichever of the requested or fallback key it ends up on.
        if !state.buckets.contains(request.key)
            && !state.buckets.has_room_for_one(now, self.max_buckets, ttl)
        {
            return Err(at_capacity("bucket", self.max_buckets, request.key));
        }
        if !state.scopes.contains(&budget.scope_key)
            && let Some(max) = self.max_buckets
        {
            state.scopes.sweep(now);
            if state.scopes.len() >= max {
                return Err(at_capacity("scope", self.max_buckets, &budget.scope_key));
            }
        }
        Ok(())
    }
}

/// Bucket and scope keys are hashes of caller-supplied material or peer
/// addresses. Neither is a secret, but neither belongs in an error body:
/// `Internal`'s payload is operator-facing, so it carries only the prefix
/// that identifies the SHAPE of the key that could not be made.
fn at_capacity(kind: &str, max: Option<usize>, key: &str) -> CratestackError {
    let end = key.find(':').map_or(0, |i| i + 1);
    let max = max.map_or_else(|| "unbounded".to_owned(), |m| m.to_string());
    CratestackError::Internal(format!(
        "rate limit store is at its {max}-{kind} cap and a sweep freed nothing, so no {kind} \
         could be created for a new caller identity (cratestack#871). Raise \
         InMemoryRateLimitStore::with_max_buckets, or move to a Redis-backed store: key={}",
        &key[..end]
    ))
}
