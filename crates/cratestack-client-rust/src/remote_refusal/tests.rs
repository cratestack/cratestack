//! cratestack#1243: a refusal's members, `details` and challenges reach the
//! caller on both transports and both codecs, decoded from hand-built
//! answers shaped exactly as the server writes them.

use cratestack_codec_cbor::CborCodec;
use cratestack_core::CratestackCodec;
use serde_json::json;

use crate::client::decode::{decode_typed_response, remote_error_from_response};
use crate::error::ClientError;
use crate::rpc::error::{RpcClientError, client_error_to_rpc, decode_rpc_unary_response};
use crate::runtime::wire::{RuntimeHeader, RuntimeResponseWire};

const RENEW: &str = r#"Signature error="attestation_expired""#;
const BEARER: &str = r#"Bearer realm="api", error="invalid_token""#;

fn header(name: &str, value: &str) -> RuntimeHeader {
    RuntimeHeader {
        name: name.to_owned(),
        value: value.to_owned(),
    }
}

/// A 401 carrying `body`, two `WWW-Authenticate` values in two spellings
/// of the name, and an unrelated header between them.
fn refusal(content_type: &str, body: Vec<u8>) -> RuntimeResponseWire {
    RuntimeResponseWire {
        status_code: 401,
        headers: vec![
            header("content-type", content_type),
            header("WWW-Authenticate", RENEW),
            header("x-request-id", "r-1"),
            header("www-authenticate", BEARER),
        ],
        body,
    }
}

fn envelope(code: &str) -> serde_json::Value {
    json!({
        "code": code,
        "message": "attestation expired",
        "details": { "device": "d-1" },
        "error": "attestation_expired",
        "retry": { "after": 3 },
    })
}

fn cbor(value: &serde_json::Value) -> Vec<u8> {
    CborCodec.encode(value).expect("encode")
}

#[cfg(feature = "codec-json")]
fn json_bytes(value: &serde_json::Value) -> Vec<u8> {
    serde_json::to_vec(value).expect("encode")
}

fn assert_rest_refusal(error: &ClientError) {
    let body = error.remote_error().expect("a decoded envelope");
    assert_eq!(body.code, "UNAUTHORIZED");
    assert_eq!(body.message, "attestation expired");
    let details = serde_json::to_value(body.details.as_ref().expect("details")).unwrap();
    assert_eq!(details, json!({ "device": "d-1" }));
    let extra = serde_json::to_value(&body.extra).unwrap();
    assert_eq!(
        extra,
        json!({ "error": "attestation_expired", "retry": { "after": 3 } })
    );
    assert_eq!(error.challenges(), [RENEW, BEARER]);
}

#[test]
fn a_rest_refusal_keeps_members_details_and_challenges_in_cbor() {
    let response = refusal(CborCodec::CONTENT_TYPE, cbor(&envelope("UNAUTHORIZED")));
    let error =
        decode_typed_response::<_, serde_json::Value>(&CborCodec, &response).expect_err("a 401");
    assert_rest_refusal(&error);
}

#[cfg(feature = "codec-json")]
#[test]
fn a_rest_refusal_keeps_members_details_and_challenges_in_json() {
    let codec = cratestack_codec_json::JsonCodec;
    let response = refusal("application/json", json_bytes(&envelope("UNAUTHORIZED")));
    let error =
        decode_typed_response::<_, serde_json::Value>(&codec, &response).expect_err("a 401");
    assert_rest_refusal(&error);
}

/// The streaming path builds its error separately; it reads the same.
#[test]
fn a_streamed_rest_refusal_reads_the_same() {
    let response = refusal(CborCodec::CONTENT_TYPE, cbor(&envelope("UNAUTHORIZED")));
    assert_rest_refusal(&remote_error_from_response(&CborCodec, &response));
}

fn assert_rpc_refusal<C: crate::codec::HttpClientCodec>(
    codec: &C,
    content_type: &str,
    body: Vec<u8>,
) {
    let response = refusal(content_type, body);
    let error =
        decode_rpc_unary_response::<_, serde_json::Value>(codec, &response).expect_err("a 401");
    let remote = error.remote().expect("a remote error");
    assert_eq!(remote.status.as_u16(), 401);
    assert_eq!(remote.body.code, "unauthenticated");
    assert_eq!(remote.body.details, Some(json!({ "device": "d-1" })));
    assert_eq!(remote.body.extra["error"], "attestation_expired");
    assert_eq!(remote.body.extra["retry"], json!({ "after": 3 }));
    assert_eq!(error.challenges(), [RENEW, BEARER]);
}

#[test]
fn an_rpc_refusal_keeps_members_details_and_challenges_in_cbor() {
    let body = cbor(&envelope("unauthenticated"));
    assert_rpc_refusal(&CborCodec, CborCodec::CONTENT_TYPE, body);
}

#[cfg(feature = "codec-json")]
#[test]
fn an_rpc_refusal_keeps_members_details_and_challenges_in_json() {
    let body = json_bytes(&envelope("unauthenticated"));
    assert_rpc_refusal(&cratestack_codec_json::JsonCodec, "application/json", body);
}

/// An RPC call that went through the REST decode path (the legacy
/// translation) keeps its challenges and members too.
#[test]
fn the_rest_to_rpc_translation_keeps_the_challenges() {
    let response = refusal(CborCodec::CONTENT_TYPE, cbor(&envelope("UNAUTHORIZED")));
    let rest = remote_error_from_response(&CborCodec, &response);
    let rpc = client_error_to_rpc(rest);
    let remote = rpc.remote().expect("a remote error");
    assert_eq!(remote.body.code, "unauthenticated");
    assert_eq!(remote.body.extra["error"], "attestation_expired");
    assert_eq!(rpc.challenges(), [RENEW, BEARER]);
}

/// Without a refusal nothing changes: no challenges, no extra members.
#[test]
fn a_plain_error_has_no_challenges_and_no_extra_members() {
    let mut response = refusal(
        CborCodec::CONTENT_TYPE,
        cbor(&json!({ "code": "UNAUTHORIZED", "message": "no credential" })),
    );
    response
        .headers
        .retain(|header| !header.name.eq_ignore_ascii_case("www-authenticate"));
    let error = remote_error_from_response(&CborCodec, &response);
    assert!(error.challenges().is_empty());
    let body = error.remote_error().expect("decoded");
    assert!(body.details.is_none() && body.extra.is_empty());
    assert!(ClientError::State("x".into()).challenges().is_empty());
    assert!(RpcClientError::BadInput("x".into()).challenges().is_empty());
}
