//! `WWW-Authenticate` on error responses (cratestack#1208).
//!
//! An [`AuthRefusal`](cratestack_core::AuthRefusal) carries its challenges
//! on the error, not in the envelope, so every encoder that turns a
//! [`CratestackError`] into a response takes them off the error first
//! ([`challenges_of`]) and puts them on the response it built ([`attach`]).
//! The RPC binding re-encodes a handler's REST-shaped error response into
//! an RPC body, and [`copy`] carries the header across that re-encoding.

use axum::http::header::WWW_AUTHENTICATE;
use axum::http::{HeaderMap, HeaderValue};
use axum::response::Response;
use cratestack_core::CratestackError;

/// The error's `WWW-Authenticate` values, taken before the error is
/// consumed by `into_response`. Empty for every error but a refusal.
pub(crate) fn challenges_of(error: &CratestackError) -> Vec<String> {
    error.auth_challenges().to_vec()
}

/// Appends each challenge as its own `WWW-Authenticate` header. A value
/// that is not a valid header value cannot reach here
/// (`AuthRefusal::with_challenge` drops it); one that does is skipped.
pub(crate) fn attach(mut response: Response, challenges: &[String]) -> Response {
    for challenge in challenges {
        if let Ok(value) = HeaderValue::from_str(challenge) {
            response.headers_mut().append(WWW_AUTHENTICATE, value);
        }
    }
    response
}

/// Copies every `WWW-Authenticate` header of `from` onto `response`.
pub(crate) fn copy(from: &HeaderMap, mut response: Response) -> Response {
    for value in from.get_all(WWW_AUTHENTICATE) {
        response
            .headers_mut()
            .append(WWW_AUTHENTICATE, value.clone());
    }
    response
}
