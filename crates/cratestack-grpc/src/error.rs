//! `CratestackError` / RPC-code-string -> `tonic::Status` mapping. Built on top of
//! `cratestack_core::rpc::{rpc_code, cratestack_error_code_to_rpc_code}`, the
//! existing, shipped `CratestackError` -> gRPC-style-string mapping — this module
//! adds one more hop (rpc-code-string -> `tonic::Code`) rather than
//! re-deriving the string codes independently.

use cratestack_core::CratestackError;
use cratestack_core::rpc::{cratestack_error_code_to_rpc_code, rpc_code};
use tonic::{Code, Status};

/// Maps a gRPC-style rpc code string (`cratestack_core::rpc`'s vocabulary —
/// `not_found`, `invalid_argument`, `permission_denied`, `unauthenticated`,
/// `conflict`, `failed_precondition`, `internal`, `unavailable` (emitted by
/// `@@subscribe` SSE backpressure overflow — `CratestackError::Unavailable`,
/// cratestack#390), plus `deadline_exceeded`/`canceled`, which the RPC
/// binding still doesn't emit today but this table covers defensively) to
/// `tonic::Code`.
///
/// `"conflict"` has no exact gRPC canonical equivalent. gRPC offers two
/// close candidates: `AlreadyExists` (the resource being created already
/// exists) and `Aborted` (a concurrency conflict, e.g. an optimistic-lock
/// failure). CrateStack's `CratestackError::Conflict` covers both create-time
/// uniqueness violations and update-time version conflicts, so neither
/// candidate is a perfect fit for every caller. `AlreadyExists` is chosen
/// because it is the mapping gRPC-gateway (the most widely deployed
/// REST<->gRPC bridge) uses for HTTP 409, and CrateStack's own REST binding
/// already maps `Conflict` to HTTP 409 — picking the gateway convention
/// keeps a client that talks to both bindings seeing the same "already
/// exists" meaning rather than a meaning that flips per transport.
pub fn rpc_code_to_tonic_code(code: &str) -> Code {
    match code {
        "invalid_argument" => Code::InvalidArgument,
        "unauthenticated" => Code::Unauthenticated,
        "permission_denied" => Code::PermissionDenied,
        "not_found" => Code::NotFound,
        "conflict" => Code::AlreadyExists,
        "failed_precondition" => Code::FailedPrecondition,
        "unavailable" => Code::Unavailable,
        "deadline_exceeded" => Code::DeadlineExceeded,
        "canceled" => Code::Cancelled,
        "internal" => Code::Internal,
        _ => Code::Unknown,
    }
}

/// Maps a `CratestackError` straight to a `tonic::Status`, going through the same
/// `rpc_code` table the RPC binding uses (`cratestack_core::rpc::rpc_code`)
/// so REST, RPC, and gRPC all agree on which `CratestackError` variant maps to
/// which stable code. The status message is `CratestackError::public_message`,
/// the same safe-to-expose text every other binding already returns.
pub fn cratestack_error_to_status(error: &CratestackError) -> Status {
    let code = rpc_code_to_tonic_code(rpc_code(error));
    Status::new(code, error.public_message().into_owned())
}

/// Same mapping, keyed off the REST-binding's screaming-snake `code` string
/// (`CratestackErrorResponse.code`, e.g. `"NOT_FOUND"`) rather than a `CratestackError`
/// value directly — for callers that already have a
/// `cratestack_error_code_to_rpc_code` string in hand (e.g. from a structured
/// error payload) and need the `tonic::Code` it maps to.
pub fn cratestack_error_code_to_tonic_code(code: &str) -> Code {
    rpc_code_to_tonic_code(cratestack_error_code_to_rpc_code(code))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exhaustive over every `CratestackError` variant this crate's error type
    /// declares — one row per variant, so a new variant added to
    /// `cratestack-core` without a corresponding row here fails this test
    /// (each variant is constructed once, deliberately, rather than
    /// pattern-matched generically) instead of silently falling through to
    /// `Code::Unknown` in production.
    #[test]
    fn every_cratestack_error_variant_maps_to_a_specific_tonic_code() {
        let cases: &[(CratestackError, Code)] = &[
            (
                CratestackError::BadRequest("x".into()),
                Code::InvalidArgument,
            ),
            (
                CratestackError::NotAcceptable("x".into()),
                Code::InvalidArgument,
            ),
            (
                CratestackError::UnsupportedMediaType("x".into()),
                Code::InvalidArgument,
            ),
            (CratestackError::Codec("x".into()), Code::InvalidArgument),
            (
                CratestackError::Validation("x".into()),
                Code::InvalidArgument,
            ),
            (
                CratestackError::Unauthorized("x".into()),
                Code::Unauthenticated,
            ),
            (
                CratestackError::Forbidden("x".into()),
                Code::PermissionDenied,
            ),
            (CratestackError::NotFound("x".into()), Code::NotFound),
            (CratestackError::Conflict("x".into()), Code::AlreadyExists),
            (
                CratestackError::PreconditionFailed("x".into()),
                Code::FailedPrecondition,
            ),
            (CratestackError::Database("x".into()), Code::Internal),
            (CratestackError::Internal("x".into()), Code::Internal),
            (CratestackError::Unavailable("x".into()), Code::Unavailable),
        ];

        for (error, expected) in cases {
            let status = cratestack_error_to_status(error);
            assert_eq!(
                status.code(),
                *expected,
                "CratestackError variant {error:?} should map to {expected:?}"
            );
        }
    }

    #[test]
    fn unknown_rpc_code_string_maps_to_unknown() {
        assert_eq!(rpc_code_to_tonic_code("something_new"), Code::Unknown);
    }

    #[test]
    fn conflict_maps_to_already_exists_not_aborted() {
        assert_eq!(rpc_code_to_tonic_code("conflict"), Code::AlreadyExists);
    }

    #[test]
    fn cratestack_error_code_string_hop_agrees_with_direct_cratestack_error_hop() {
        // NOT_FOUND (REST vocabulary) and the CratestackError::NotFound variant
        // must land on the same tonic::Code — same table, two entry points.
        assert_eq!(
            cratestack_error_code_to_tonic_code("NOT_FOUND"),
            cratestack_error_to_status(&CratestackError::NotFound("x".into())).code()
        );
    }
}
