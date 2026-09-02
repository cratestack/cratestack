//! Synthesise a [`CratestackError`] from a bare HTTP status when the body
//! isn't a recognised error shape.

use cratestack_core::CratestackError;

pub(super) fn synthesize_error_for_status(status: axum::http::StatusCode) -> CratestackError {
    let code = status.as_u16();
    let suffix = format!("upstream returned {code}");
    match code {
        400 => CratestackError::BadRequest(suffix),
        401 => CratestackError::Unauthorized(suffix),
        403 => CratestackError::Forbidden(suffix),
        404 => CratestackError::NotFound(suffix),
        406 => CratestackError::NotAcceptable(suffix),
        409 => CratestackError::Conflict(suffix),
        412 => CratestackError::PreconditionFailed(suffix),
        415 => CratestackError::UnsupportedMediaType(suffix),
        422 => CratestackError::Validation(suffix),
        // cratestack#846: without this arm a throttle that reached the
        // dispatcher as a bare 429 (a handler or an inner layer that
        // emitted no recognised body) was re-synthesised as `Internal`,
        // i.e. re-labelled `internal` on the wire while keeping status
        // 429 — the two halves of the frame disagreeing about what
        // happened.
        429 => CratestackError::TooManyRequests(suffix),
        503 => CratestackError::Unavailable(suffix),
        _ => CratestackError::Internal(suffix),
    }
}
