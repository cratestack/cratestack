//! Turning a [`ReservationOutcome`] into either "we hold the
//! reservation, run the handler" or a finished response.
//!
//! Split out of `service.rs` (cratestack#846) to keep that file under the
//! workspace's 200-line ceiling once every error exit grew a
//! headers/path argument. Behaviour is unchanged; the four arms moved
//! verbatim.

use axum::response::Response;
use cratestack_core::CratestackError;
use http::HeaderMap;

use crate::middleware_error::middleware_error_response;

use super::record::ReservationOutcome;
use super::responses::{in_flight_response, replay_response};

/// Deliberately not a `Result`: a replay is a *success*, not an error,
/// and `Result<Uuid, Response>` trips `clippy::result_large_err` besides.
pub(super) enum Reservation {
    /// This caller won the reservation and must run the handler.
    Held(uuid::Uuid),
    /// Nothing left to run — return this response as-is.
    Finished(Response),
}

pub(super) fn token_or_response(
    outcome: ReservationOutcome,
    headers: &HeaderMap,
    path: &str,
) -> Reservation {
    match outcome {
        ReservationOutcome::Replay(record) => Reservation::Finished(replay_response(&record)),
        ReservationOutcome::Conflict => Reservation::Finished(middleware_error_response(
            headers,
            path,
            CratestackError::Validation(
                "idempotency_key_conflict: key reused with a different request body".to_owned(),
            ),
        )),
        ReservationOutcome::InFlight => Reservation::Finished(in_flight_response(headers, path)),
        ReservationOutcome::Reserved { token } => Reservation::Held(token),
    }
}
