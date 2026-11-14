//! An `err` frame keeps its RPC code through the client's error types.

use cratestack_core::rpc::RpcErrorBody;

use super::rpc_error_to_client;
use crate::rpc::error::client_error_to_rpc;
use crate::{RpcClientError, RpcRemoteError};

fn read(code: &str, details: Option<serde_json::Value>) -> RpcRemoteError {
    let body = RpcErrorBody {
        code: code.to_owned(),
        message: "refused".to_owned(),
        details,
        extra: Default::default(),
    };
    match client_error_to_rpc(rpc_error_to_client(body)) {
        RpcClientError::Remote(remote) => remote,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn a_throttle_err_frame_keeps_its_code_status_and_details() {
    let details = serde_json::json!({"retry_after_secs": 3});
    let remote = read("resource_exhausted", Some(details.clone()));
    assert_eq!(remote.body.code, "resource_exhausted");
    assert_eq!(remote.status, 429);
    assert_eq!(remote.body.details, Some(details));
}

#[test]
fn every_rpc_code_survives_an_err_frame() {
    for (code, status) in [
        ("invalid_argument", 400),
        ("unauthenticated", 401),
        ("permission_denied", 403),
        ("not_found", 404),
        ("conflict", 409),
        ("aborted", 409),
        ("failed_precondition", 412),
        ("unavailable", 503),
        ("internal", 500),
    ] {
        let remote = read(code, None);
        assert_eq!(remote.body.code, code);
        assert_eq!(remote.status, status, "{code}");
    }
}

#[cfg(all(
    any(feature = "ws", feature = "webtransport"),
    not(target_arch = "wasm32")
))]
mod socket_items {
    use cratestack_core::rpc::RpcFrameValue;
    use serde::Deserialize;

    use super::super::cbor_item;

    const ID: u128 = 0x0123_4567_89ab_cdef_0123_4567_89ab_cdef;

    #[derive(Debug, Deserialize, PartialEq)]
    struct Blob {
        id: uuid::Uuid,
        extra: cratestack_core::Value,
    }

    fn object(id: RpcFrameValue, extra: RpcFrameValue) -> RpcFrameValue {
        RpcFrameValue::Object([("id".to_owned(), id), ("extra".to_owned(), extra)].into())
    }

    #[test]
    fn a_byte_string_uuid_and_bytes_decode_as_a_unary_cbor_call_does() {
        let value = object(
            RpcFrameValue::Bytes(ID.to_be_bytes().to_vec()),
            RpcFrameValue::Bytes(vec![9, 8]),
        );
        let blob: Blob = cbor_item(value).expect("a CBOR-form item");
        assert_eq!(blob.id, uuid::Uuid::from_u128(ID));
        assert_eq!(blob.extra, cratestack_core::Value::Bytes(vec![9, 8]));
    }

    #[test]
    fn a_text_uuid_from_an_older_server_still_reads() {
        let value = object(
            RpcFrameValue::String(uuid::Uuid::from_u128(ID).to_string()),
            RpcFrameValue::String("CQg=".to_owned()),
        );
        let blob: Blob = cbor_item(value).expect("the human-readable fallback");
        assert_eq!(blob.id, uuid::Uuid::from_u128(ID));
        assert_eq!(
            blob.extra,
            cratestack_core::Value::String("CQg=".to_owned())
        );
    }

    #[test]
    fn an_item_neither_way_reads_reports_the_cbor_error() {
        let error = cbor_item::<Blob>(RpcFrameValue::Bool(true)).unwrap_err();
        assert!(error.to_string().contains("subscription item"), "{error}");
    }
}
