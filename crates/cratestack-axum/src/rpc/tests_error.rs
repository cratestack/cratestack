//! Error-code mapping + error-body shaping tests.

#![cfg(test)]

use cratestack_core::CratestackError;
use cratestack_core::rpc::{RpcErrorBody, cratestack_error_code_to_rpc_code, rpc_code};

#[test]
fn cratestack_error_code_to_rpc_code_covers_every_cratestack_error_variant() {
    // Mirror image of `rpc_code_maps_each_cratestack_error_variant` — for
    // every CratestackError variant, encoding it as CratestackErrorResponse and
    // then translating its `code` must land on the same gRPC-style
    // string as the direct `rpc_code` path.
    for variant in [
        CratestackError::BadRequest("x".into()),
        CratestackError::NotAcceptable("x".into()),
        CratestackError::Unauthorized("x".into()),
        CratestackError::UnsupportedMediaType("x".into()),
        CratestackError::Forbidden("x".into()),
        CratestackError::NotFound("x".into()),
        CratestackError::Conflict("x".into()),
        CratestackError::Validation("x".into()),
        CratestackError::PreconditionFailed("x".into()),
        CratestackError::Codec("x".into()),
        CratestackError::Database("x".into()),
        CratestackError::Internal("x".into()),
        CratestackError::Unavailable("x".into()),
    ] {
        let cratestack_code = variant.code();
        let direct = rpc_code(&variant);
        let translated = cratestack_error_code_to_rpc_code(cratestack_code);
        assert_eq!(
            direct, translated,
            "rpc_code({:?}) = {:?} but cratestack_error_code_to_rpc_code({:?}) = {:?}",
            variant, direct, cratestack_code, translated,
        );
    }
}

#[test]
fn cratestack_error_code_to_rpc_code_unknown_input_falls_to_internal() {
    // A server that adds a new CratestackError variant we don't know about
    // shouldn't leak a SCREAMING string to the wire — degrade to
    // "internal" rather than passing through.
    assert_eq!(
        cratestack_error_code_to_rpc_code("SOMETHING_NEW"),
        "internal"
    );
    assert_eq!(cratestack_error_code_to_rpc_code(""), "internal");
}

#[test]
fn error_body_from_cratestack_response_translates_code_and_preserves_message() {
    let response = cratestack_core::CratestackErrorResponse {
        code: "NOT_FOUND".to_owned(),
        message: "widget 42".to_owned(),
        details: None,
    };
    let body = RpcErrorBody::from_cratestack_response(response);
    assert_eq!(body.code, "not_found");
    assert_eq!(body.message, "widget 42");
    assert!(body.details.is_none());
}

#[test]
fn rpc_code_maps_each_cratestack_error_variant() {
    assert_eq!(
        rpc_code(&CratestackError::BadRequest("x".into())),
        "invalid_argument"
    );
    assert_eq!(
        rpc_code(&CratestackError::NotAcceptable("x".into())),
        "invalid_argument"
    );
    assert_eq!(
        rpc_code(&CratestackError::Unauthorized("x".into())),
        "unauthenticated"
    );
    assert_eq!(
        rpc_code(&CratestackError::UnsupportedMediaType("x".into())),
        "invalid_argument",
    );
    assert_eq!(
        rpc_code(&CratestackError::Forbidden("x".into())),
        "permission_denied"
    );
    assert_eq!(
        rpc_code(&CratestackError::NotFound("x".into())),
        "not_found"
    );
    assert_eq!(rpc_code(&CratestackError::Conflict("x".into())), "conflict");
    assert_eq!(
        rpc_code(&CratestackError::Validation("x".into())),
        "invalid_argument"
    );
    assert_eq!(
        rpc_code(&CratestackError::PreconditionFailed("x".into())),
        "failed_precondition",
    );
    assert_eq!(
        rpc_code(&CratestackError::Codec("x".into())),
        "invalid_argument"
    );
    assert_eq!(rpc_code(&CratestackError::Database("x".into())), "internal");
    assert_eq!(rpc_code(&CratestackError::Internal("x".into())), "internal");
    assert_eq!(
        rpc_code(&CratestackError::Unavailable("x".into())),
        "unavailable"
    );
    // cratestack#846: `RateLimitLayer`'s throttle. gRPC's canonical code
    // for an exhausted quota, and the code the REST screaming-snake
    // `TOO_MANY_REQUESTS` translates to — the two vocabularies must not
    // drift, or a throttle would decode differently per transport.
    assert_eq!(
        rpc_code(&CratestackError::TooManyRequests("x".into())),
        "resource_exhausted"
    );
    assert_eq!(
        cratestack_core::rpc::cratestack_error_code_to_rpc_code("TOO_MANY_REQUESTS"),
        "resource_exhausted"
    );
}

#[test]
fn error_body_uses_public_message_not_operator_detail() {
    // 5xx variants must return the canned public message, never the
    // operator-only detail string carried inside the variant.
    let body = RpcErrorBody::from_cratestack(&CratestackError::Internal("db ip refused".into()));
    assert_eq!(body.code, "internal");
    assert_eq!(body.message, "internal error");
    assert!(
        !body.message.contains("db ip refused"),
        "internal error detail leaked to the wire: {}",
        body.message,
    );
}

#[test]
fn error_body_uses_caller_supplied_message_for_4xx() {
    let body = RpcErrorBody::from_cratestack(&CratestackError::NotFound("widget 42".into()));
    assert_eq!(body.code, "not_found");
    assert_eq!(body.message, "widget 42");
}
