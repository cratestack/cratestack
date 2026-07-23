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
    assert_eq!(
        err.detail(),
        Some("malformed CBOR major type 7 at offset 42")
    );
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
fn parse_cuid_accepts_cuid_v1_style_id() {
    // Legacy cuid v1 ids: 'c' prefix + lowercase alphanumeric, 25 chars.
    let id = "ch72gsb320000udocl363eofy";
    assert_eq!(parse_cuid(id).unwrap(), id);
}

#[test]
fn parse_cuid_accepts_cuid2_style_id() {
    // cuid2 ids: 24 lowercase-alphanumeric chars, no fixed leading 'c'.
    let id = "go17t93z1vbd99yl5toj7eu5";
    let result = parse_cuid(id);
    assert_eq!(result.unwrap(), id);
}

#[test]
fn parse_cuid_rejects_empty_string() {
    assert!(parse_cuid("").is_err());
}

#[test]
fn parse_cuid_rejects_uppercase_chars() {
    assert!(parse_cuid("Go17t93z1vbd99yl5toj7eu5").is_err());
}

#[test]
fn parse_cuid_rejects_non_alphanumeric_chars() {
    assert!(parse_cuid("go17t93z1vbd-9yl5toj7eu5").is_err());
    assert!(parse_cuid("go17t93z1vbd_9yl5toj7eu5").is_err());
    assert!(parse_cuid("go17t93z1vbd 9yl5toj7eu5").is_err());
}

#[test]
fn parse_cuid_rejects_single_char() {
    assert!(parse_cuid("a").is_err());
}

#[test]
fn parse_cuid_rejects_absurdly_long_string() {
    let too_long = "a".repeat(64);
    assert!(parse_cuid(&too_long).is_err());
}

#[test]
fn parse_cuid_accepts_boundary_lengths() {
    assert!(parse_cuid("ab").is_ok());
    assert!(parse_cuid(&"a".repeat(32)).is_ok());
    assert!(parse_cuid(&"a".repeat(33)).is_err());
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
