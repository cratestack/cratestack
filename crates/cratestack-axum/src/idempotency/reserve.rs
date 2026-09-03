//! Turning an [`Admission`] into either "run the handler" or a finished
//! response.
//!
//! Split out of `service.rs` (cratestack#846) to keep that file under the
//! workspace's 200-line ceiling once every error exit grew a
//! headers/path argument. Behaviour is unchanged; the four arms moved
//! verbatim then, and moved verbatim again in ADR 0015 slice 1 when the
//! matched type changed from `ReservationOutcome` to [`Admission`] — the
//! two mirror each other one for one precisely so this stayed a rename.

use axum::response::Response;
use cratestack_core::CratestackError;
use cratestack_exec::{Admission, OpAdmission, OpExecutor, OpInput};
use http::HeaderMap;

use crate::middleware_error::middleware_error_response;

use super::responses::{in_flight_response, replay_response};

/// Deliberately not a `Result`: a replay is a *success*, not an error,
/// and `Result<Uuid, Response>` trips `clippy::result_large_err` besides.
pub(super) enum Reservation {
    /// This caller won the reservation and must run the handler, then
    /// complete or release it under this token.
    Held(uuid::Uuid),
    /// Run the handler with nothing to complete or release afterwards —
    /// the op declared `idempotent_by_default`, or no key was supplied,
    /// or no store is wired. Joins [`Held`](Reservation::Held) in the
    /// run-the-handler branch and differs only in owing no follow-up.
    Bypass,
    /// Nothing left to run — return this response as-is.
    Finished(Response),
}

/// Ask L3 for an admission decision and render whatever it says.
///
/// The whole HTTP→L3 boundary is these ten lines: an [`OpInput`] built
/// entirely from values `IdempotencyService` already had in hand, and a
/// store error rendered through the same `middleware_error_response` it
/// always was. `ctx` is `None` — slice 3 fills it.
pub(super) async fn admit_or_response(
    executor: &OpExecutor,
    op: OpAdmission,
    principal: &str,
    key: &str,
    fingerprint: [u8; 32],
    headers: &HeaderMap,
    path: &str,
) -> Reservation {
    // `OpInput::new` rather than a struct literal: the type is
    // `#[non_exhaustive]` so slice 3 can add its context field without a
    // second breaking release. `ctx` stays unset here — `with_ctx` is the
    // call site slice 3 will add.
    let input = OpInput::new(op, principal, Some(key), fingerprint);
    match executor.admit(&input).await {
        Ok(admission) => token_or_response(admission, headers, path),
        Err(error) => Reservation::Finished(middleware_error_response(headers, path, error)),
    }
}

fn token_or_response(admission: Admission, headers: &HeaderMap, path: &str) -> Reservation {
    match admission {
        Admission::Replay(record) => Reservation::Finished(replay_response(&record)),
        Admission::Conflict => Reservation::Finished(middleware_error_response(
            headers,
            path,
            CratestackError::Validation(
                "idempotency_key_conflict: key reused with a different request body".to_owned(),
            ),
        )),
        Admission::InFlight => Reservation::Finished(in_flight_response(headers, path)),
        Admission::Reserved { token } => Reservation::Held(token),
        Admission::Bypass => Reservation::Bypass,
        // `Admission` is `#[non_exhaustive]`; slices 2 and 3 add variants
        // (a rate-limit refusal, a policy denial). Refusing is the only
        // safe default: this adapter cannot know what a future outcome
        // permits, and the two arms it might otherwise fall into —
        // `Held` and `Bypass` — both RUN THE HANDLER. A 500 on an
        // outcome we do not understand is a bug report; silently
        // executing one is a duplicate charge.
        //
        // Deliberately not interpolated into the message: `Replay`
        // carries a captured response body, and a future variant may
        // too — an error string is the wrong place for either.
        _ => Reservation::Finished(middleware_error_response(
            headers,
            path,
            CratestackError::Internal(
                "idempotency: unhandled admission outcome; refusing rather than \
                 running the operation"
                    .to_owned(),
            ),
        )),
    }
}
