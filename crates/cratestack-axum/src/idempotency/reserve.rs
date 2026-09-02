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

/// `Ok(token)` means this caller won the reservation and must run the
/// handler; `Err(response)` is the finished response to return as-is.
pub(super) fn token_or_response(
    outcome: ReservationOutcome,
    headers: &HeaderMap,
    path: &str,
) -> Result<uuid::Uuid, Response> {
    match outcome {
        ReservationOutcome::Replay(record) => Err(replay_response(&record)),
        ReservationOutcome::Conflict => Err(middleware_error_response(
            headers,
            path,
            CratestackError::Validation(
                "idempotency_key_conflict: key reused with a different request body".to_owned(),
            ),
        )),
        ReservationOutcome::InFlight => Err(in_flight_response(headers, path)),
        ReservationOutcome::Reserved { token } => Ok(token),
    }
}
