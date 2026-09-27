//! The [`CratestackError`] → RPC code maps: from the error itself
//! ([`rpc_code`]) and from the REST-binding code string a handler's error
//! response carries ([`cratestack_error_code_to_rpc_code`]). The two must
//! agree; `cratestack-axum`'s `tests_error.rs` checks every variant.

use crate::error::CratestackError;

/// Map a [`CratestackError`] to its stable RPC code (gRPC-style snake_case).
pub const fn rpc_code(error: &CratestackError) -> &'static str {
    match error {
        CratestackError::BadRequest(_)
        | CratestackError::NotAcceptable(_)
        | CratestackError::UnsupportedMediaType(_)
        | CratestackError::Codec(_)
        | CratestackError::Validation(_) => "invalid_argument",
        CratestackError::Unauthorized(_) => "unauthenticated",
        CratestackError::Forbidden(_) => "permission_denied",
        CratestackError::NotFound(_) => "not_found",
        CratestackError::Conflict(_) | CratestackError::ConflictTyped(_) => "conflict",
        // gRPC's canonical code for "aborted, typically by a concurrency
        // issue such as a transaction abort" (google.rpc.Code.ABORTED = 10):
        // an `@isolation` procedure ran out of retries after serialization
        // failures. Distinct from `conflict`, which a retry repeats.
        CratestackError::TransactionAborted(_) => "aborted",
        CratestackError::PreconditionFailed(_) => "failed_precondition",
        CratestackError::Database(_)
        | CratestackError::DatabaseTyped(_)
        | CratestackError::Internal(_) => "internal",
        CratestackError::Unavailable(_) => "unavailable",
        // gRPC's canonical code for "the caller exhausted a quota/rate
        // limit" (google.rpc.Code.RESOURCE_EXHAUSTED = 8). New with
        // cratestack#846's `TooManyRequests`; no prior variant mapped here.
        CratestackError::TooManyRequests(_) => "resource_exhausted",
    }
}

/// Map a `CratestackErrorResponse.code` string (screaming-snake, REST-
/// binding vocabulary) to the stable gRPC-style code the RPC binding
/// emits.
pub fn cratestack_error_code_to_rpc_code(code: &str) -> &'static str {
    match code {
        "BAD_REQUEST"
        | "NOT_ACCEPTABLE"
        | "UNSUPPORTED_MEDIA_TYPE"
        | "VALIDATION_ERROR"
        | "CODEC_ERROR" => "invalid_argument",
        "UNAUTHORIZED" => "unauthenticated",
        "FORBIDDEN" => "permission_denied",
        "NOT_FOUND" => "not_found",
        "CONFLICT" => "conflict",
        "TRANSACTION_ABORTED" => "aborted",
        "PRECONDITION_FAILED" => "failed_precondition",
        "DATABASE_ERROR" | "INTERNAL_ERROR" => "internal",
        "UNAVAILABLE" => "unavailable",
        "TOO_MANY_REQUESTS" => "resource_exhausted",
        _ => "internal",
    }
}
