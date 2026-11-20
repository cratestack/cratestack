use super::*;
use crate::CratestackError;

#[test]
fn every_reason_round_trips_its_wire_name() {
    for reason in StreamEndReason::ALL {
        assert_eq!(StreamEndReason::parse(reason.as_str()), Some(reason));
        let json = serde_json::to_string(&reason).unwrap();
        assert_eq!(json, format!("\"{}\"", reason.as_str()));
    }
    assert_eq!(StreamEndReason::parse("nope"), None);
}

#[test]
fn status_code_and_rpc_code_follow_the_reason_table() {
    let table = [
        (StreamEndReason::CredentialExpired, 401, "unauthenticated"),
        (StreamEndReason::StreamLifetime, 503, "unavailable"),
        (StreamEndReason::SessionEnded, 401, "unauthenticated"),
        (
            StreamEndReason::PrincipalSuspended,
            403,
            "permission_denied",
        ),
        (StreamEndReason::DeviceRevoked, 403, "permission_denied"),
        (StreamEndReason::RevocationResync, 503, "unavailable"),
    ];
    for (reason, status, rpc) in table {
        let error = StreamEndNotice::new(reason).into_error();
        assert_eq!(error.status_code().as_u16(), status, "{reason:?}");
        assert_eq!(crate::rpc::rpc_code(&error), rpc, "{reason:?}");
    }
}

#[test]
fn the_two_pre_existing_ends_keep_their_messages() {
    assert_eq!(
        StreamEndNotice::new(StreamEndReason::CredentialExpired).message,
        "credential expired"
    );
    assert_eq!(
        StreamEndNotice::new(StreamEndReason::StreamLifetime).message,
        "stream lifetime"
    );
}

#[test]
fn the_rest_envelope_carries_the_reason_in_details() {
    let response =
        CratestackError::from(StreamEndNotice::new(StreamEndReason::DeviceRevoked)).into_response();
    assert_eq!(response.code, "FORBIDDEN");
    assert_eq!(response.message, "device revoked");
    let Some(Value::Map(details)) = response.details else {
        panic!("details missing");
    };
    assert_eq!(
        details.get("reason"),
        Some(&Value::String("device_revoked".into()))
    );
}
