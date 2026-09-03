//! The executor itself.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use cratestack_core::idempotency_record::ReservationOutcome;
use cratestack_core::{CratestackError, IdempotencyStore};

use crate::admission::Admission;
use crate::input::OpInput;

/// Runs the admission half of an operation, independently of how the
/// caller arrived.
///
/// # Why the store is an `Option`
///
/// It collapses two situations that are genuinely the same situation into
/// one code path: a `db = None` service (which has no store to wire) and a
/// server whose operator simply did not install one. Both mean "there is
/// nowhere to record a reservation", both must run the op rather than
/// refuse it, and neither is an error. Making the store mandatory would
/// have forced a second constructor, or a null-object store, to say the
/// same thing less clearly.
///
/// # Named collaborators, not a lookup
///
/// Both fields are supplied at construction and read by name. Per ADR 0012
/// there is no registry and no type-keyed resolution here — see this
/// crate's module doc for why that constraint is about the dependency
/// graph being *inspectable*, not about taste.
#[derive(Clone)]
pub struct OpExecutor {
    idempotency: Option<Arc<dyn IdempotencyStore>>,
    ttl: Duration,
}

impl OpExecutor {
    /// Wire the executor to a store, or to nothing.
    ///
    /// `idempotency` is `None` for a service with no store to install —
    /// `db = None`, or an operator who simply did not configure one. Both
    /// admit everything; see the type's own docs for why those two are the
    /// same path rather than two.
    ///
    /// `ttl` bounds a reservation's lifetime. It is read once per
    /// [`admit`](Self::admit) — against the clock at that moment, not at
    /// construction — so a long-lived executor does not hand out
    /// reservations that expire relative to process start.
    pub fn new(idempotency: Option<Arc<dyn IdempotencyStore>>, ttl: Duration) -> Self {
        Self { idempotency, ttl }
    }

    /// Decide whether this call may run, and whether it owes a
    /// completion.
    ///
    /// The three bypass tests are ordered cheapest-first and are all
    /// pure; the store is only touched once none of them fired. Order is
    /// not observable — they are disjoint conditions — but it does mean a
    /// `@no_idempotency` op never reaches the store at all, which is what
    /// the "reserve call count 0 vs 1" test asserts.
    pub async fn admit(&self, input: &OpInput<'_>) -> Result<Admission, CratestackError> {
        let Some(store) = self.idempotency.as_ref() else {
            return Ok(Admission::Bypass);
        };
        let Some(key) = input.idempotency_key else {
            return Ok(Admission::Bypass);
        };
        if input.op.idempotent_by_default {
            return Ok(Admission::Bypass);
        }

        // Computed here rather than accepted as a parameter so every
        // caller gets the same TTL semantics from the same clock read;
        // this is the line the HTTP adapter used to own.
        let expires_at = SystemTime::now() + self.ttl;
        let outcome = store
            .reserve_or_fetch(input.principal, key, input.fingerprint, expires_at)
            .await?;
        Ok(match outcome {
            ReservationOutcome::Reserved { token } => Admission::Reserved { token },
            ReservationOutcome::Replay(record) => Admission::Replay(record),
            ReservationOutcome::InFlight => Admission::InFlight,
            ReservationOutcome::Conflict => Admission::Conflict,
        })
    }

    /// Record the op's outcome against a reservation this executor
    /// granted, so later calls under the same key replay it.
    ///
    /// Infallible by return type, matching the store contract it wraps:
    /// the op has already run and its effects already happened, so a
    /// failure to persist the *record* of it must not turn a successful
    /// call into an error response. A token that no longer owns the row
    /// (the TTL lapsed and a retry re-reserved) is silently ignored by
    /// the store, which is what stops a slow handler from poisoning the
    /// newer reservation.
    pub async fn complete(
        &self,
        principal: &str,
        key: &str,
        token: uuid::Uuid,
        status: u16,
        headers: &[u8],
        body: &[u8],
    ) {
        let Some(store) = self.idempotency.as_ref() else {
            return;
        };
        let _ = store
            .complete(principal, key, token, status, headers, body)
            .await;
    }

    /// Give up a reservation without recording an outcome, so a retry can
    /// re-acquire it instead of seeing `InFlight` until the TTL lapses.
    /// Best-effort for the same reason as [`Self::complete`].
    pub async fn release(&self, principal: &str, key: &str, token: uuid::Uuid) {
        let Some(store) = self.idempotency.as_ref() else {
            return;
        };
        let _ = store.release(principal, key, token).await;
    }
}
