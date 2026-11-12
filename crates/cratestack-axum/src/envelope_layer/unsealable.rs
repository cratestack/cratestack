//! The refusal of an unsigned request whose reply the layer cannot seal,
//! under `EnvelopeMode::UnsignedRequestSealedReply` (cratestack#1313).

use axum::response::Response;
use cratestack_core::CratestackError;
use http::{HeaderMap, StatusCode};

use crate::middleware_error::middleware_error_response_with_status;

/// Why the reply could not be sealed.
#[derive(Debug, Clone, Copy)]
pub(super) enum Unsealable {
    /// No nonce, two, or a malformed one: the reply could not be bound.
    Nonce,
    /// `Accept` (or the seal policy) does not ask for a sealed reply.
    Accept,
    /// A subscription streams, and a stream cannot be sealed yet (P1).
    Stream,
}

/// The unsigned `406`, sent before the handler runs so nothing happened that
/// the caller could not authenticate the answer to. Unsigned like every
/// refusal made before verification (D4): there is no request digest to
/// bind it to.
pub(super) fn refuse(headers: &HeaderMap, path: &str, why: Unsealable) -> Response {
    let message = match why {
        Unsealable::Nonce => {
            "this operation answers only sealed replies: send exactly one valid \
             Cratestack-Nonce header"
        }
        Unsealable::Accept => {
            "this operation answers only sealed replies: Accept must name application/cose"
        }
        Unsealable::Stream => "a streamed response cannot be sealed yet (ADR 0006 P1)",
    };
    middleware_error_response_with_status(
        headers,
        path,
        StatusCode::NOT_ACCEPTABLE,
        CratestackError::NotAcceptable(message.to_owned()),
    )
}
