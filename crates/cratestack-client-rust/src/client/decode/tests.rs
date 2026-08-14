//! Unit coverage for `decode_typed_response`/`decode_typed_response_with_metadata`
//! against hand-built `RuntimeResponseWire` values — no real HTTP server
//! needed, since these functions never touch the network themselves.
//! End-to-end coverage of the real `GET` → `ETag` → `PATCH` `If-Match`
//! round trip (over a real HTTP connection, through the public
//! `*_with_response` methods) lives in `tests/typed_response.rs`.

use cratestack_codec_cbor::CborCodec;
use cratestack_core::CratestackCodec;
use serde::{Deserialize, Serialize};

use super::{decode_typed_response, decode_typed_response_with_metadata};
use crate::error::ClientError;
use crate::runtime::wire::{RuntimeHeader, RuntimeResponseWire};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Ledger {
    id: i64,
    balance: i64,
}

fn cbor_response(
    status_code: u16,
    headers: Vec<RuntimeHeader>,
    value: &Ledger,
) -> RuntimeResponseWire {
    let mut headers = headers;
    headers.push(RuntimeHeader {
        name: "content-type".to_owned(),
        value: CborCodec::CONTENT_TYPE.to_owned(),
    });
    RuntimeResponseWire {
        status_code,
        headers,
        body: CborCodec.encode(value).expect("value should encode"),
    }
}

/// Proves the additive path survives status + headers — including an
/// `ETag`-shaped header — that the original path discards.
#[test]
fn decode_typed_response_with_metadata_preserves_status_and_etag_header() {
    let ledger = Ledger { id: 4, balance: 1 };
    let response = cbor_response(
        200,
        vec![RuntimeHeader {
            name: "ETag".to_owned(),
            value: "\"0\"".to_owned(),
        }],
        &ledger,
    );

    let typed: super::TypedResponse<Ledger> =
        decode_typed_response_with_metadata(&CborCodec, &response).expect("should decode");

    assert_eq!(typed.value, ledger);
    assert_eq!(typed.status.as_u16(), 200);
    assert_eq!(typed.header("etag"), Some("\"0\""));
    // Case-insensitive lookup, matching how a real server might send
    // either casing.
    assert_eq!(typed.header("ETAG"), Some("\"0\""));
}

/// The original `decode_typed_response` signature and behavior are
/// unchanged: still just the value, nothing else — proving the
/// refactor onto `decode_typed_response_with_metadata` didn't leak any
/// new requirement onto existing call sites.
#[test]
fn decode_typed_response_still_returns_only_the_value() {
    let ledger = Ledger { id: 7, balance: 42 };
    let response = cbor_response(
        200,
        vec![RuntimeHeader {
            name: "ETag".to_owned(),
            value: "\"3\"".to_owned(),
        }],
        &ledger,
    );

    let value: Ledger =
        decode_typed_response(&CborCodec, &response).expect("should decode to bare value");

    assert_eq!(value, ledger);
}

/// A non-2xx response maps to `ClientError::Remote` identically on
/// both the plain and metadata-preserving decode paths — the new
/// function doesn't change error classification.
#[test]
fn decode_typed_response_with_metadata_maps_error_status_like_the_plain_path() {
    let error_body = cratestack_core::CratestackErrorResponse {
        code: "PRECONDITION_FAILED".to_owned(),
        message: "stale If-Match".to_owned(),
        details: None,
    };
    let response = RuntimeResponseWire {
        status_code: 412,
        headers: vec![RuntimeHeader {
            name: "content-type".to_owned(),
            value: CborCodec::CONTENT_TYPE.to_owned(),
        }],
        body: CborCodec.encode(&error_body).expect("value should encode"),
    };

    let error = decode_typed_response_with_metadata::<_, Ledger>(&CborCodec, &response)
        .expect_err("412 should surface as an error");

    match error {
        ClientError::Remote { status, error, .. } => {
            assert_eq!(status.as_u16(), 412);
            assert_eq!(
                error.expect("error body should decode").code,
                "PRECONDITION_FAILED"
            );
        }
        other => panic!("expected ClientError::Remote, got {other:?}"),
    }
}
