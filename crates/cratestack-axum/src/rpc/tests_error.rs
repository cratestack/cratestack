//! Error-code mapping + error-body shaping tests.

#![cfg(test)]

use cratestack_core::CoolError;
use cratestack_core::rpc::{RpcErrorBody, cool_error_code_to_rpc_code, rpc_code};

#[test]
fn cool_error_code_to_rpc_code_covers_every_cool_error_variant() {
    // Mirror image of `rpc_code_maps_each_cool_error_variant` — for
    // every CoolError variant, encoding it as CoolErrorResponse and
    // then translating its `code` must land on the same gRPC-style
    // string as the direct `rpc_code` path.
    for variant in [
        CoolError::BadRequest("x".into()),
        CoolError::NotAcceptable("x".into()),
        CoolError::Unauthorized("x".into()),
        CoolError::UnsupportedMediaType("x".into()),
        CoolError::Forbidden("x".into()),
        CoolError::NotFound("x".into()),
        CoolError::Conflict("x".into()),
        CoolError::Validation("x".into()),
        CoolError::PreconditionFailed("x".into()),
        CoolError::Codec("x".into()),
        CoolError::Database("x".into()),
        CoolError::Internal("x".into()),
    ] {
        let cool_code = variant.code();
        let direct = rpc_code(&variant);
        let translated = cool_error_code_to_rpc_code(cool_code);
        assert_eq!(
            direct, translated,
            "rpc_code({:?}) = {:?} but cool_error_code_to_rpc_code({:?}) = {:?}",
            variant, direct, cool_code, translated,
        );
    }
}

#[test]
fn cool_error_code_to_rpc_code_unknown_input_falls_to_internal() {
    // A server that adds a new CoolError variant we don't know about
    // shouldn't leak a SCREAMING string to the wire — degrade to
    // "internal" rather than passing through.
    assert_eq!(cool_error_code_to_rpc_code("SOMETHING_NEW"), "internal");
    assert_eq!(cool_error_code_to_rpc_code(""), "internal");
}

#[test]
fn error_body_from_cool_response_translates_code_and_preserves_message() {
    let response = cratestack_core::CoolErrorResponse {
        code: "NOT_FOUND".to_owned(),
        message: "widget 42".to_owned(),
        details: None,
    };
    let body = RpcErrorBody::from_cool_response(response);
    assert_eq!(body.code, "not_found");
    assert_eq!(body.message, "widget 42");
    assert!(body.details.is_none());
}

#[test]
fn rpc_code_maps_each_cool_error_variant() {
    assert_eq!(rpc_code(&CoolError::BadRequest("x".into())), "invalid_argument");
    assert_eq!(rpc_code(&CoolError::NotAcceptable("x".into())), "invalid_argument");
    assert_eq!(rpc_code(&CoolError::Unauthorized("x".into())), "unauthenticated");
    assert_eq!(
        rpc_code(&CoolError::UnsupportedMediaType("x".into())),
        "invalid_argument",
    );
    assert_eq!(rpc_code(&CoolError::Forbidden("x".into())), "permission_denied");
    assert_eq!(rpc_code(&CoolError::NotFound("x".into())), "not_found");
    assert_eq!(rpc_code(&CoolError::Conflict("x".into())), "conflict");
    assert_eq!(rpc_code(&CoolError::Validation("x".into())), "invalid_argument");
    assert_eq!(
        rpc_code(&CoolError::PreconditionFailed("x".into())),
        "failed_precondition",
    );
    assert_eq!(rpc_code(&CoolError::Codec("x".into())), "invalid_argument");
    assert_eq!(rpc_code(&CoolError::Database("x".into())), "internal");
    assert_eq!(rpc_code(&CoolError::Internal("x".into())), "internal");
}

#[test]
fn error_body_uses_public_message_not_operator_detail() {
    // 5xx variants must return the canned public message, never the
    // operator-only detail string carried inside the variant.
    let body = RpcErrorBody::from_cool(&CoolError::Internal("db ip refused".into()));
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
    let body = RpcErrorBody::from_cool(&CoolError::NotFound("widget 42".into()));
    assert_eq!(body.code, "not_found");
    assert_eq!(body.message, "widget 42");
}
