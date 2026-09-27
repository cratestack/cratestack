//! The payload of [`CratestackError::TransactionAborted`] and who owns it
//! (docs/design/procedure-isolation.md §5, §6; GHSA-r67q-4qqq-g9gm).
//!
//! "Retries exhausted, nothing committed" is true of the transaction whose
//! retry loop gave up — not of every caller the error travels through. A
//! procedure without `@isolation` that committed a debit on the pool and
//! then propagated another procedure's `TRANSACTION_ABORTED` with `?` has
//! committed work: releasing its `Idempotency-Key` would let the same key
//! debit again. So the error carries where it stands, and only one state is
//! releasable: [`AbortOwnership::Claimed`], set by the generated dispatch of
//! the `@isolation` procedure whose own retry loop produced it. It is also
//! the only state a response may answer as `TRANSACTION_ABORTED`: any other
//! is answered as `INTERNAL_ERROR`
//! ([`CratestackError::disowned_transaction_abort`]).
//!
//! Because `Claimed` releases a key, the type keeps it out of reach of code
//! that is not the generated dispatch: `ownership` is private, the public
//! constructors build only `Exhausted` or `Propagated`, and the one way to
//! `Claimed` is [`CratestackError::__generated_claim_transaction_abort`]
//! (docs/design/procedure-isolation.md §6; the compile-time proof is
//! `aborted_doctests.rs`).

use super::{CratestackError, DbErrorInfo};

/// Where a [`CratestackError::TransactionAborted`] stands relative to the
/// procedure dispatch that answers the request. Decides
/// [`CratestackError::is_idempotency_replayable`] and what the response says
/// ([`CratestackError::disowned_transaction_abort`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum AbortOwnership {
    /// Returned by the retry loop whose own attempts ran out, and not (yet)
    /// claimed by the dispatch of the procedure that owns that loop — what
    /// a caller of `<procedure>::invoke_with_db` receives. Answered as a 500
    /// `INTERNAL_ERROR` and recorded by an idempotency layer, like any other
    /// error.
    #[default]
    Exhausted,
    /// Passed out of a procedure body: another procedure's exhausted
    /// transaction, propagated by a caller that may have committed work of
    /// its own. Answered as a 500 `INTERNAL_ERROR`, recorded, and never
    /// claimed.
    Propagated,
    /// Claimed by the generated dispatch (REST, RPC, MCP) of the
    /// `@isolation` procedure whose own retries ran out: nothing the call
    /// did was committed. The only state answered as 409
    /// `TRANSACTION_ABORTED`, and the only one an idempotency layer
    /// releases. Reached only through
    /// [`CratestackError::__generated_claim_transaction_abort`].
    Claimed,
}

/// Payload of [`CratestackError::TransactionAborted`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct TransactionAbort {
    /// The fixed public detail and the SQLSTATE (`40001`/`40P01`) that
    /// exhausted the retries. `constraint` is always `None`.
    pub info: DbErrorInfo,
    /// Private: read it with [`Self::ownership`]. Only
    /// [`CratestackError::__generated_claim_transaction_abort`] sets
    /// [`AbortOwnership::Claimed`].
    ownership: AbortOwnership,
}

impl TransactionAbort {
    /// The abort of the retry loop whose own attempts ran out, not claimed
    /// by any dispatch: [`AbortOwnership::Exhausted`].
    pub fn exhausted(info: DbErrorInfo) -> Self {
        Self {
            info,
            ownership: AbortOwnership::Exhausted,
        }
    }

    /// Another transaction's abort, passed out of a procedure body:
    /// [`AbortOwnership::Propagated`].
    pub fn propagated(info: DbErrorInfo) -> Self {
        Self {
            info,
            ownership: AbortOwnership::Propagated,
        }
    }

    /// See [`AbortOwnership`].
    pub fn ownership(&self) -> AbortOwnership {
        self.ownership
    }
}

impl CratestackError {
    /// Whether an idempotency layer may record this outcome and replay it to
    /// a later call under the same `Idempotency-Key`. `false` only for a
    /// [`Self::TransactionAborted`] whose ownership is
    /// [`AbortOwnership::Claimed`]: the answering dispatch owns the
    /// transaction that ran out of retries and nothing was committed, so the
    /// layer releases the reservation instead and the same key can be sent
    /// again. Any other `TransactionAborted` — one a caller propagated out of
    /// its own body — is recorded like every other error, because that
    /// caller may have committed work before propagating it.
    pub fn is_idempotency_replayable(&self) -> bool {
        !matches!(
            self,
            Self::TransactionAborted(TransactionAbort {
                ownership: AbortOwnership::Claimed,
                ..
            })
        )
    }

    /// **Generated code only.** The generated dispatch (REST, RPC, MCP) of
    /// an `@isolation` procedure calls this on its `invoke_with_db` result:
    /// an [`AbortOwnership::Exhausted`] abort there is the dispatch's own
    /// (bodies' errors were already marked [`AbortOwnership::Propagated`]
    /// by the retry loop), so it becomes [`AbortOwnership::Claimed`]. Every
    /// other error is returned unchanged.
    ///
    /// It is the only way to `Claimed`, which releases the request's
    /// `Idempotency-Key`. It is public only because generated code lives in
    /// the consuming crate. Hand-written code that calls it claims an abort
    /// after work of its own that may have committed, and so releases the
    /// key of the request it is answering, and only that one; that is a
    /// contract, not a boundary (docs/design/procedure-isolation.md §6).
    #[doc(hidden)]
    pub fn __generated_claim_transaction_abort(self) -> Self {
        match self {
            Self::TransactionAborted(TransactionAbort {
                info,
                ownership: AbortOwnership::Exhausted,
            }) => Self::TransactionAborted(TransactionAbort {
                info,
                ownership: AbortOwnership::Claimed,
            }),
            other => other,
        }
    }

    /// What a response answers in place of this error when it is a
    /// [`Self::TransactionAborted`] the answering dispatch does not own —
    /// any ownership but [`AbortOwnership::Claimed`]: an `Internal` (500
    /// `INTERNAL_ERROR`) carrying the original detail for the operator's log.
    /// `None` for every other error, a claimed abort included.
    ///
    /// `TRANSACTION_ABORTED` tells a client that nothing was committed and
    /// to send the request again. A non-owner (a procedure that propagated
    /// another transaction's abort, possibly after committing work of its
    /// own) cannot say that: a retry under a new key could apply that work
    /// twice (docs/design/procedure-isolation.md §6). The HTTP encoders and
    /// MCP's tool and resource paths apply it to every error they answer.
    #[doc(hidden)]
    pub fn disowned_transaction_abort(&self) -> Option<Self> {
        match self {
            Self::TransactionAborted(abort) if abort.ownership != AbortOwnership::Claimed => {
                Some(Self::Internal(format!(
                    "a transaction aborted by concurrent updates (SQLSTATE {}) reached a \
                     dispatch that does not own it ({:?}); answered as an internal error: {}",
                    abort.info.sqlstate.as_deref().unwrap_or("none"),
                    abort.ownership,
                    abort.info.detail,
                )))
            }
            _ => None,
        }
    }

    /// The retry loop calls this on an error a procedure body returned: a
    /// `TransactionAborted` coming out of a body is another transaction's,
    /// never this loop's, so it becomes [`AbortOwnership::Propagated`] and
    /// can no longer be claimed. Every other error is returned unchanged.
    #[doc(hidden)]
    pub fn propagate_transaction_abort(self) -> Self {
        match self {
            Self::TransactionAborted(abort) => {
                Self::TransactionAborted(TransactionAbort::propagated(abort.info))
            }
            other => other,
        }
    }
}
