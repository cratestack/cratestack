//! The answer [`crate::OpExecutor::admit`] gives: may this call run, and
//! does the caller owe a `complete`/`release` afterwards?

use cratestack_core::idempotency_record::IdempotencyRecord;

/// Outcome of an admission decision.
///
/// The last four variants mirror
/// [`cratestack_core::idempotency_record::ReservationOutcome`] one for one,
/// on purpose: the HTTP adapter's mapping from store outcome to response
/// is then a rename, not a re-derivation, and the four response arms it
/// already had (`replay_response`, `idempotency_key_conflict`,
/// `in_flight_response`, run-the-handler) move verbatim. A shape that
/// collapsed or reordered them would have made "the wire did not change"
/// an argument instead of an inspection.
///
/// [`Bypass`](Admission::Bypass) is the one genuinely new variant, and the
/// only one that carries no token.
///
/// # `#[non_exhaustive]`
///
/// Slices 2 and 3 add variants — a rate-limit refusal carries a retry
/// budget, and policy denial is a different answer from either — and this
/// crate is unreleased, so the marker costs nothing now and saves a second
/// breaking release later. It forces a wildcard arm on external `match`es;
/// `cratestack-axum`'s is deliberately fail-closed (it refuses the request
/// rather than running the handler), because the one thing a future
/// admission outcome must never do by default is silently admit. See
/// `idempotency/reserve.rs`.
#[non_exhaustive]
pub enum Admission {
    /// Run the op with no reservation taken: there is nothing to
    /// `complete` and nothing to `release`.
    ///
    /// Four disjoint reasons produce it, and only the first is new
    /// behaviour:
    ///
    /// 1. the op declares `idempotent_by_default` — a read, a pure
    ///    procedure, or a mutation the schema marked `@no_idempotency`;
    /// 2. the caller supplied no idempotency key;
    /// 3. no [`cratestack_core::IdempotencyStore`] is wired up, which is
    ///    also what a `db = None` service looks like from here;
    /// 4. (decided by the caller, before it ever builds an
    ///    [`crate::OpInput`]) the call is not on an idempotency-target
    ///    method — a transport fact, so the HTTP adapter still owns it.
    Bypass,
    /// This caller claimed the key and must run the op, then call
    /// [`crate::OpExecutor::complete`] or
    /// [`crate::OpExecutor::release`] with this token.
    Reserved { token: uuid::Uuid },
    /// A previous call under this key + fingerprint already finished.
    /// Return its recorded outcome; do not run the op.
    Replay(IdempotencyRecord),
    /// Another caller is running under this key + fingerprint right now.
    InFlight,
    /// The key was claimed by a *different* request body — the IETF
    /// draft's `idempotency_key_conflict`.
    Conflict,
}
