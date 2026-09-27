//! Error responses the idempotency layer must not record
//! (GHSA-r67q-4qqq-g9gm, docs/design/procedure-isolation.md §6).
//!
//! The layer sees a `Response`, not the `CratestackError` it came from, and
//! decoding the body would mean negotiating every codec. So the encoders that
//! turn a handler's error into a response tag it instead: a zero-sized
//! response extension, never sent on the wire, set exactly when
//! [`CratestackError::is_idempotency_replayable`] is `false`: an
//! `@isolation` procedure's own dispatch answering that its retries ran out
//! — nothing was committed. The layer then releases the reservation rather
//! than completing it, so the same `Idempotency-Key` can be sent again and
//! runs the call again.
//!
//! A `TRANSACTION_ABORTED` the dispatch did not claim (a caller propagating
//! another procedure's abort, after possibly committing work of its own) is
//! not answered as one at all: [`answered`] turns it into a 500
//! `INTERNAL_ERROR`, which is recorded like any other error. Telling that
//! client "nothing was committed, send it again" could make a retry under a
//! new key apply its work twice.
//!
//! RPC's error re-encoding (`crate::rpc::convert_handler_error_response`)
//! carries the tag across. `/rpc/batch` refuses an `Idempotency-Key` header
//! outright, so no batch response reaches the layer with a reservation.

use axum::response::Response;
use cratestack_core::CratestackError;

/// The tag. See the module docs.
#[derive(Debug, Clone, Copy)]
pub(crate) struct UnrecordedOutcome;

/// The error a response answers with, and the tag that response needs, if
/// any. Taken before the error is consumed into its response body.
pub(crate) fn answered(error: CratestackError) -> (CratestackError, Option<UnrecordedOutcome>) {
    let error = disowned(&error).unwrap_or(error);
    let tag = unrecorded_tag(&error);
    (error, tag)
}

/// The tag the response to an already-[`answered`] `error` needs, if any.
pub(crate) fn unrecorded_tag(error: &CratestackError) -> Option<UnrecordedOutcome> {
    (!error.is_idempotency_replayable()).then_some(UnrecordedOutcome)
}

/// The `INTERNAL_ERROR` a response answers instead of `error`, when `error`
/// is a `TRANSACTION_ABORTED` the answering dispatch does not own
/// ([`CratestackError::disowned_transaction_abort`]). Logs the original.
pub(crate) fn disowned(error: &CratestackError) -> Option<CratestackError> {
    let internal = error.disowned_transaction_abort()?;
    tracing::warn!(
        target: "cratestack",
        cratestack_error = error.code(),
        cratestack_sqlstate = error.db_sqlstate().unwrap_or(""),
        cratestack_detail = internal.detail().unwrap_or(""),
        "a TRANSACTION_ABORTED the answering dispatch does not own is answered as INTERNAL_ERROR",
    );
    Some(internal)
}

/// `response` with `tag` applied.
pub(crate) fn with_tag(tag: Option<UnrecordedOutcome>, mut response: Response) -> Response {
    if let Some(tag) = tag {
        response.extensions_mut().insert(tag);
    }
    response
}

/// The tag `response` carries, if any.
pub(crate) fn tag_of(response: &Response) -> Option<UnrecordedOutcome> {
    response.extensions().get::<UnrecordedOutcome>().copied()
}
