//! `ClientError::stream_end_reason` over the frames the server writes.

use cratestack_core::StreamEndReason;
use cratestack_core::rpc::RpcErrorBody;
use serde_json::json;

use crate::error::ClientError;
use crate::rpc_error_to_client;

fn frame(code: &str, details: Option<serde_json::Value>) -> ClientError {
    let mut body = json!({ "code": code, "message": "ended" });
    if let Some(details) = details {
        body["details"] = details;
    }
    rpc_error_to_client(serde_json::from_value::<RpcErrorBody>(body).expect("an err body"))
}

#[test]
fn every_reason_the_server_sends_is_read_back() {
    let cases = [
        (
            "unauthenticated",
            "credential_expired",
            StreamEndReason::CredentialExpired,
        ),
        (
            "unavailable",
            "stream_lifetime",
            StreamEndReason::StreamLifetime,
        ),
        (
            "unauthenticated",
            "session_ended",
            StreamEndReason::SessionEnded,
        ),
        (
            "permission_denied",
            "principal_suspended",
            StreamEndReason::PrincipalSuspended,
        ),
        (
            "permission_denied",
            "device_revoked",
            StreamEndReason::DeviceRevoked,
        ),
        (
            "unavailable",
            "revocation_resync",
            StreamEndReason::RevocationResync,
        ),
    ];
    for (code, name, expected) in cases {
        let error = frame(code, Some(json!({ "reason": name })));
        assert_eq!(error.stream_end_reason(), Some(expected), "{name}");
    }
}

#[test]
fn an_unknown_reason_is_none_and_keeps_the_code_behaviour() {
    let error = frame("unavailable", Some(json!({ "reason": "from_the_future" })));
    assert_eq!(error.stream_end_reason(), None);
    let ClientError::Remote { status, .. } = &error else {
        panic!("a remote error");
    };
    assert_eq!(*status, reqwest::StatusCode::SERVICE_UNAVAILABLE);
}

#[test]
fn absent_or_malformed_details_are_none() {
    assert_eq!(frame("unavailable", None).stream_end_reason(), None);
    assert_eq!(
        frame("unavailable", Some(json!({}))).stream_end_reason(),
        None
    );
    assert_eq!(
        frame("unavailable", Some(json!({ "reason": 7 }))).stream_end_reason(),
        None
    );
    assert_eq!(
        frame("unavailable", Some(json!("session_ended"))).stream_end_reason(),
        None
    );
    assert_eq!(ClientError::State("x".into()).stream_end_reason(), None);
}
