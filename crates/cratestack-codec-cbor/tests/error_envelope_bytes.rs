//! cratestack#1208 gave the error envelopes a hand-written serde so they
//! can carry extra members. That must not change a single byte of an
//! envelope without them: these encode each one next to a copy of the
//! struct as the derive used to write it, and compare the CBOR bytes.

use cratestack_codec_cbor::CborCodec;
use cratestack_core::rpc::RpcErrorBody;
use cratestack_core::{CratestackCodec, CratestackError, CratestackErrorResponse, Value};
use serde::Serialize;

/// `CratestackErrorResponse` as `#[derive(Serialize)]` wrote it.
#[derive(Serialize)]
struct DerivedResponse {
    code: String,
    message: String,
    details: Option<Value>,
}

/// `RpcErrorBody` as `#[derive(Serialize)]` wrote it.
#[derive(Serialize)]
struct DerivedRpcBody {
    code: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<serde_json::Value>,
}

#[test]
fn a_rest_envelope_without_members_encodes_as_before() {
    for details in [None, Some(Value::String("why".into()))] {
        let response = CratestackErrorResponse {
            code: "NOT_FOUND".into(),
            message: "widget".into(),
            details: details.clone(),
            extra: Default::default(),
        };
        let derived = DerivedResponse {
            code: "NOT_FOUND".into(),
            message: "widget".into(),
            details,
        };
        assert_eq!(
            CborCodec.encode(&response).unwrap(),
            CborCodec.encode(&derived).unwrap()
        );
    }
}

#[test]
fn an_rpc_body_without_members_encodes_as_before() {
    for details in [None, Some(serde_json::json!({ "field": "name" }))] {
        let body = RpcErrorBody {
            code: "not_found".into(),
            message: "widget".into(),
            details: details.clone(),
            extra: Default::default(),
        };
        let derived = DerivedRpcBody {
            code: "not_found".into(),
            message: "widget".into(),
            details,
        };
        assert_eq!(
            CborCodec.encode(&body).unwrap(),
            CborCodec.encode(&derived).unwrap()
        );
    }
}

#[test]
fn a_refusal_round_trips_through_cbor() {
    let error: CratestackError = cratestack_core::AuthRefusal::unauthenticated("expired")
        .with_member("error", Value::String("attestation_expired".into()))
        .into_error();
    let bytes = CborCodec.encode(&error.into_response()).unwrap();
    let back: CratestackErrorResponse = CborCodec.decode(&bytes).unwrap();
    assert_eq!(back.code, "UNAUTHORIZED");
    assert_eq!(
        back.extra["error"],
        Value::String("attestation_expired".into())
    );
    let rpc = RpcErrorBody::from_cratestack_response(back);
    let back: RpcErrorBody = CborCodec.decode(&CborCodec.encode(&rpc).unwrap()).unwrap();
    assert_eq!(back, rpc);
}
