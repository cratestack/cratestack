use super::*;

#[test]
fn internal_error_public_message_does_not_leak_detail() {
    let secret = "SELECT * FROM accounts WHERE pan = '4111-1111-1111-1111'";
    let err = CoolError::Internal(secret.to_owned());
    let response = err.into_response();
    assert_eq!(response.code, "INTERNAL_ERROR");
    assert_eq!(response.message, "internal error");
    assert!(
        !response.message.contains("SELECT"),
        "5xx public message must not echo internal detail",
    );
    assert!(
        !response.message.contains("4111"),
        "5xx public message must not echo any sensitive substring",
    );
    assert!(response.details.is_none());
}

#[test]
fn database_error_public_message_is_canned() {
    let err = CoolError::Database("FATAL: connection refused at db.internal:5432".to_owned());
    assert_eq!(err.public_message(), "internal error");
    assert_eq!(
        err.detail(),
        Some("FATAL: connection refused at db.internal:5432")
    );
}

#[test]
fn codec_error_public_message_is_canned() {
    let err = CoolError::Codec("malformed CBOR major type 7 at offset 42".to_owned());
    assert_eq!(err.public_message(), "invalid request payload");
    assert_eq!(err.detail(), Some("malformed CBOR major type 7 at offset 42"));
}

#[test]
fn client_error_public_message_passes_through_caller_string() {
    let err = CoolError::BadRequest("missing query parameter 'limit'".to_owned());
    let response = err.into_response();
    assert_eq!(response.code, "BAD_REQUEST");
    assert_eq!(response.message, "missing query parameter 'limit'");
}

#[test]
fn precondition_failed_maps_to_412() {
    let err = CoolError::PreconditionFailed("stale ETag".to_owned());
    assert_eq!(err.status_code(), StatusCode::PRECONDITION_FAILED);
    assert_eq!(err.code(), "PRECONDITION_FAILED");
    let response = err.into_response();
    assert_eq!(response.message, "stale ETag");
}

#[test]
fn detail_is_none_for_empty_string() {
    let err = CoolError::Internal(String::new());
    assert_eq!(err.detail(), None);
}

#[test]
fn into_response_never_populates_details_field() {
    for err in [
        CoolError::BadRequest("x".to_owned()),
        CoolError::Validation("y".to_owned()),
        CoolError::Internal("z".to_owned()),
        CoolError::Database("w".to_owned()),
        CoolError::Codec("v".to_owned()),
        CoolError::PreconditionFailed("u".to_owned()),
    ] {
        let response = err.into_response();
        assert!(
            response.details.is_none(),
            "details field must remain None until structured details are introduced",
        );
    }
}
