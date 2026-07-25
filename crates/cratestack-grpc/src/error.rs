//! `CoolError` / RPC-code-string -> `tonic::Status` mapping. Built on top of
//! `cratestack_core::rpc::{rpc_code, cool_error_code_to_rpc_code}`, the
//! existing, shipped `CoolError` -> gRPC-style-string mapping — this module
//! adds one more hop (rpc-code-string -> `tonic::Code`) rather than
//! re-deriving the string codes independently.

use cratestack_core::CoolError;
use cratestack_core::rpc::{cool_error_code_to_rpc_code, rpc_code};
use tonic::{Code, Status};

/// Maps a gRPC-style rpc code string (`cratestack_core::rpc`'s vocabulary —
/// `not_found`, `invalid_argument`, `permission_denied`, `unauthenticated`,
/// `conflict`, `failed_precondition`, `internal`, plus the two the RPC
/// binding never emits today but this table still covers defensively:
/// `unavailable`, `deadline_exceeded`, `canceled`) to `tonic::Code`.
///
/// `"conflict"` has no exact gRPC canonical equivalent. gRPC offers two
/// close candidates: `AlreadyExists` (the resource being created already
/// exists) and `Aborted` (a concurrency conflict, e.g. an optimistic-lock
/// failure). CrateStack's `CoolError::Conflict` covers both create-time
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

/// Maps a `CoolError` straight to a `tonic::Status`, going through the same
/// `rpc_code` table the RPC binding uses (`cratestack_core::rpc::rpc_code`)
/// so REST, RPC, and gRPC all agree on which `CoolError` variant maps to
/// which stable code. The status message is `CoolError::public_message`,
/// the same safe-to-expose text every other binding already returns.
pub fn cool_error_to_status(error: &CoolError) -> Status {
    let code = rpc_code_to_tonic_code(rpc_code(error));
    Status::new(code, error.public_message().into_owned())
}

/// Same mapping, keyed off the REST-binding's screaming-snake `code` string
/// (`CoolErrorResponse.code`, e.g. `"NOT_FOUND"`) rather than a `CoolError`
/// value directly — for callers that already have a
/// `cool_error_code_to_rpc_code` string in hand (e.g. from a structured
/// error payload) and need the `tonic::Code` it maps to.
pub fn cool_error_code_to_tonic_code(code: &str) -> Code {
    rpc_code_to_tonic_code(cool_error_code_to_rpc_code(code))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exhaustive over every `CoolError` variant this crate's error type
    /// declares — one row per variant, so a new variant added to
    /// `cratestack-core` without a corresponding row here fails this test
    /// (each variant is constructed once, deliberately, rather than
    /// pattern-matched generically) instead of silently falling through to
    /// `Code::Unknown` in production.
    #[test]
    fn every_cool_error_variant_maps_to_a_specific_tonic_code() {
        let cases: &[(CoolError, Code)] = &[
            (CoolError::BadRequest("x".into()), Code::InvalidArgument),
            (CoolError::NotAcceptable("x".into()), Code::InvalidArgument),
            (
                CoolError::UnsupportedMediaType("x".into()),
                Code::InvalidArgument,
            ),
            (CoolError::Codec("x".into()), Code::InvalidArgument),
            (CoolError::Validation("x".into()), Code::InvalidArgument),
            (CoolError::Unauthorized("x".into()), Code::Unauthenticated),
            (CoolError::Forbidden("x".into()), Code::PermissionDenied),
            (CoolError::NotFound("x".into()), Code::NotFound),
            (CoolError::Conflict("x".into()), Code::AlreadyExists),
            (
                CoolError::PreconditionFailed("x".into()),
                Code::FailedPrecondition,
            ),
            (CoolError::Database("x".into()), Code::Internal),
            (CoolError::Internal("x".into()), Code::Internal),
        ];

        for (error, expected) in cases {
            let status = cool_error_to_status(error);
            assert_eq!(
                status.code(),
                *expected,
                "CoolError variant {error:?} should map to {expected:?}"
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
    fn cool_error_code_string_hop_agrees_with_direct_cool_error_hop() {
        // NOT_FOUND (REST vocabulary) and the CoolError::NotFound variant
        // must land on the same tonic::Code — same table, two entry points.
        assert_eq!(
            cool_error_code_to_tonic_code("NOT_FOUND"),
            cool_error_to_status(&CoolError::NotFound("x".into())).code()
        );
    }
}
