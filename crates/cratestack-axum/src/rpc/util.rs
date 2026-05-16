//! Synthesise a [`CoolError`] from a bare HTTP status when the body
//! isn't a recognised error shape.

use cratestack_core::CoolError;

pub(super) fn synthesize_error_for_status(status: axum::http::StatusCode) -> CoolError {
    let code = status.as_u16();
    let suffix = format!("upstream returned {code}");
    match code {
        400 => CoolError::BadRequest(suffix),
        401 => CoolError::Unauthorized(suffix),
        403 => CoolError::Forbidden(suffix),
        404 => CoolError::NotFound(suffix),
        406 => CoolError::NotAcceptable(suffix),
        409 => CoolError::Conflict(suffix),
        412 => CoolError::PreconditionFailed(suffix),
        415 => CoolError::UnsupportedMediaType(suffix),
        422 => CoolError::Validation(suffix),
        _ => CoolError::Internal(suffix),
    }
}
