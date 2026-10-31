//! Which refusals an idempotency layer releases instead of recording
//! (cratestack#1261), and who may say so.
//!
//! A 401 or 403 under an `Idempotency-Key` used to be recorded and replayed
//! like any other error. But the documented answer to one is to renew the
//! credential, or step up (ADR 0022, RFC 9470 §3), and send **the same
//! request again under the same key**; with the refusal recorded, that retry
//! got the stored 401 back forever.
//!
//! Releasing a key is safe only when nothing was committed
//! (GHSA-r67q-4qqq-g9gm, docs/design/procedure-isolation.md §6). That is
//! true of the refusals the generated dispatch produces before any body
//! runs: `AuthProvider::authenticate`'s, the `@allow`/`@deny` check's, the
//! `@authn` requirement's (`require_authn`) and an `@authorize` model
//! check's. It is not true of a refusal a procedure body returns, possibly
//! after a write. So, as with [`AbortOwnership`](super::AbortOwnership),
//! the refusal carries who produced it, and only one state is released: an
//! [`AuthRefusal`] whose [`AuthRefusal::is_dispatch_owned`] is `true`, set
//! by [`CratestackError::__generated_claim_auth_refusal`] at those generated
//! call sites and nowhere else. A plain [`CratestackError::Unauthorized`]
//! or [`CratestackError::Forbidden`] is claimed by becoming an
//! `AuthRefused` with the same code, status, message and no members,
//! details or challenges, so the response is byte for byte the one it
//! replaces.

use super::{AuthRefusal, CratestackError};

impl CratestackError {
    /// **Generated code only.** The generated dispatch (REST, RPC, MCP)
    /// calls this on a refusal produced before any procedure body ran: its
    /// `AuthProvider::authenticate` call, and its procedure's authorization
    /// step (`@allow`/`@deny`, `@authn`, `@authorize`). A 401 or 403 —
    /// [`Self::Unauthorized`], [`Self::Forbidden`] or [`Self::AuthRefused`]
    /// — becomes an [`AuthRefusal`] that [`AuthRefusal::is_dispatch_owned`],
    /// which [`Self::is_idempotency_replayable`] reads as "release the key".
    /// Every other error is returned unchanged.
    ///
    /// Public only because generated code lives in the consuming crate.
    /// Hand-written code that calls it on a refusal after work of its own
    /// releases the key of the request it is answering, and only that one:
    /// a contract, not a boundary, as with
    /// [`Self::__generated_claim_transaction_abort`].
    #[doc(hidden)]
    pub fn __generated_claim_auth_refusal(self) -> Self {
        let refusal = match self {
            Self::Unauthorized(message) => AuthRefusal::unauthenticated(message),
            Self::Forbidden(message) => AuthRefusal::forbidden(message),
            Self::AuthRefused(refusal) => refusal,
            other => return other,
        };
        Self::AuthRefused(refusal.owned_by_dispatch(true))
    }

    /// A procedure's generated `invoke_with_db` calls this on every error
    /// its body returned: a refusal coming out of a body is the body's,
    /// never the dispatch's, even when the body propagated one it got from
    /// another procedure's generated dispatch path. Every other error is
    /// returned unchanged.
    #[doc(hidden)]
    pub fn disown_auth_refusal(self) -> Self {
        match self {
            Self::AuthRefused(refusal) if refusal.is_dispatch_owned() => {
                Self::AuthRefused(refusal.owned_by_dispatch(false))
            }
            other => other,
        }
    }

    /// Whether this is a refusal the generated dispatch claimed
    /// ([`Self::__generated_claim_auth_refusal`]).
    pub(super) fn is_dispatch_owned_refusal(&self) -> bool {
        matches!(self, Self::AuthRefused(refusal) if refusal.is_dispatch_owned())
    }
}
