//! A typed stream end (cratestack#1381) in the error maps and the wire body.

#![cfg(test)]

use cratestack_core::rpc::{RpcErrorBody, cratestack_error_code_to_rpc_code, rpc_code};
use cratestack_core::{CratestackError, StreamEndNotice, StreamEndReason};

fn stream_end(reason: StreamEndReason) -> CratestackError {
    StreamEndNotice::new(reason).into_error()
}

#[test]
fn the_rest_code_and_the_rpc_code_agree_for_every_reason() {
    for reason in [
        StreamEndReason::CredentialExpired,
        StreamEndReason::StreamLifetime,
        StreamEndReason::SessionEnded,
        StreamEndReason::PrincipalSuspended,
        StreamEndReason::DeviceRevoked,
        StreamEndReason::RevocationResync,
    ] {
        let variant = stream_end(reason);
        assert_eq!(
            rpc_code(&variant),
            cratestack_error_code_to_rpc_code(variant.code()),
            "{reason:?}"
        );
    }
}

#[test]
fn an_error_body_carries_the_reason_in_details() {
    let body = RpcErrorBody::from_cratestack(&stream_end(StreamEndReason::PrincipalSuspended));
    assert_eq!(body.code, "permission_denied");
    assert_eq!(body.message, "principal suspended");
    assert_eq!(
        body.details,
        Some(serde_json::json!({ "reason": "principal_suspended" }))
    );
    // The two ends that existed before keep their code and message.
    let expired = RpcErrorBody::from_cratestack(&stream_end(StreamEndReason::CredentialExpired));
    assert_eq!(
        (expired.code.as_str(), expired.message.as_str()),
        ("unauthenticated", "credential expired")
    );
    let lifetime = RpcErrorBody::from_cratestack(&stream_end(StreamEndReason::StreamLifetime));
    assert_eq!(
        (lifetime.code.as_str(), lifetime.message.as_str()),
        ("unavailable", "stream lifetime")
    );
}
