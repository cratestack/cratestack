//! cratestack#1208: `AuthRefusal`, and the envelopes that carry its extra
//! members (`CratestackErrorResponse`, `RpcErrorBody`).

use std::collections::BTreeMap;

use super::*;
use crate::rpc::{RpcErrorBody, rpc_code};
use crate::value::Value;

fn renew() -> AuthRefusal {
    AuthRefusal::unauthenticated("attestation expired")
        .with_member("error", Value::String("attestation_expired".into()))
        .with_challenge(r#"Signature error="attestation_expired""#)
}

#[test]
fn a_refusal_keeps_the_fixed_codes_of_its_kind() {
    let unauthenticated = renew().into_error();
    assert_eq!(unauthenticated.code(), "UNAUTHORIZED");
    assert_eq!(unauthenticated.status_code(), StatusCode::UNAUTHORIZED);
    assert_eq!(rpc_code(&unauthenticated), "unauthenticated");
    let forbidden = AuthRefusal::forbidden("no").into_error();
    assert_eq!(forbidden.code(), "FORBIDDEN");
    assert_eq!(forbidden.status_code(), StatusCode::FORBIDDEN);
    assert_eq!(rpc_code(&forbidden), "permission_denied");
}

#[test]
fn reserved_names_and_bad_challenges_are_dropped() {
    let refusal = AuthRefusal::unauthenticated("m")
        .with_member("code", Value::Null)
        .with_member("message", Value::Null)
        .with_member("details", Value::Null)
        .with_challenge("Bearer\nerror=\"x\"")
        .with_challenge("Bearer café");
    assert!(refusal.members().is_empty());
    assert!(refusal.challenges().is_empty());
}

#[test]
fn the_envelope_carries_members_after_code_message_and_details() {
    let error = renew().into_error();
    assert_eq!(
        error.auth_challenges(),
        [r#"Signature error="attestation_expired""#]
    );
    let json = serde_json::to_string(&error.into_response()).unwrap();
    assert_eq!(
        json,
        r#"{"code":"UNAUTHORIZED","message":"attestation expired","details":null,"error":"attestation_expired"}"#,
    );
}

/// An envelope without extra members is written exactly as the derived
/// serde wrote it before cratestack#1208.
#[test]
fn an_envelope_without_members_is_unchanged() {
    let error = CratestackError::NotFound("widget".into());
    assert!(error.auth_challenges().is_empty());
    let json = serde_json::to_string(&error.into_response()).unwrap();
    assert_eq!(
        json,
        r#"{"code":"NOT_FOUND","message":"widget","details":null}"#
    );
    let rpc = RpcErrorBody::from_cratestack(&CratestackError::NotFound("widget".into()));
    let json = serde_json::to_string(&rpc).unwrap();
    assert_eq!(json, r#"{"code":"not_found","message":"widget"}"#);
}

#[test]
fn a_reserved_name_put_in_extra_by_hand_is_never_written() {
    let response = CratestackErrorResponse {
        code: "UNAUTHORIZED".into(),
        message: "m".into(),
        details: None,
        extra: BTreeMap::from([("code".to_owned(), Value::String("HIJACKED".into()))]),
    };
    let json = serde_json::to_string(&response).unwrap();
    assert_eq!(
        json,
        r#"{"code":"UNAUTHORIZED","message":"m","details":null}"#
    );
}

#[test]
fn envelopes_read_back_unknown_members_into_extra() {
    let response: CratestackErrorResponse = serde_json::from_str(
        r#"{"code":"UNAUTHORIZED","message":"m","error":"attestation_expired"}"#,
    )
    .unwrap();
    assert!(response.details.is_none());
    assert_eq!(
        response.extra["error"],
        Value::String("attestation_expired".into())
    );
    let body = RpcErrorBody::from_cratestack_response(response);
    assert_eq!(body.code, "unauthenticated");
    assert_eq!(body.extra["error"], "attestation_expired");
    let back: RpcErrorBody = serde_json::from_str(&serde_json::to_string(&body).unwrap()).unwrap();
    assert_eq!(back, body);
}

#[test]
fn an_envelope_without_code_or_message_does_not_decode() {
    assert!(serde_json::from_str::<CratestackErrorResponse>(r#"{"message":"m"}"#).is_err());
    assert!(serde_json::from_str::<RpcErrorBody>(r#"{"code":"x"}"#).is_err());
}
