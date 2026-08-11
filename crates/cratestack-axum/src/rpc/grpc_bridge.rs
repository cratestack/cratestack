//! Bridges a macro-generated `_dispatch` fn's `axum::response::Response`
//! into a value a `transport grpc` service method can hand back to tonic —
//! ticket #171's "no second dispatch path" requirement: gRPC method bodies
//! call the exact same `handle_*_dispatch` fns REST/RPC already call, then
//! use this module to turn the resulting `Response` into either the
//! decoded domain value or the REST-style `(code, message)` pair
//! `cratestack_grpc::cool_error_code_to_tonic_code` maps to a
//! `tonic::Status`.
//!
//! Lives here (not in `cratestack-grpc`) because it needs `axum::Response`,
//! `HttpTransport`, and [`decode_rpc_body`] — this crate already owns all
//! three; `cratestack-grpc` deliberately stays axum-agnostic (its own
//! module docs: "runtime primitives... prost/tonic re-exports", no axum
//! dependency). `cratestack-grpc`'s error module supplies the code->status
//! mapping; this function only needs to get the caller to a plain
//! `(code, message)` pair, not `tonic::Status` itself, so this crate
//! doesn't need a `tonic` dependency either.

use axum::http::HeaderMap;
use axum::response::Response;
use cratestack_core::CoolErrorResponse;
use serde::de::DeserializeOwned;

use super::codec_helpers::decode_rpc_body;
use crate::transport::HttpTransport;

/// `headers` must be the *same* `HeaderMap` passed to the dispatch call
/// that produced `response` — no `Content-Type`/`Accept` set, so content
/// negotiation resolves to the same default codec on both the dispatch
/// side and this decode side (mirrors [`decode_rpc_body`]'s own
/// `DEFAULT_CONTENT_TYPE` fallback, reused unchanged here).
///
/// `Ok(value)` on a 2xx dispatch response, decoded straight to `T`.
/// `Err((code, message))` otherwise — either the dispatch response's own
/// `CoolErrorResponse` (its `code` is `cratestack_core::CoolError::code()`'s
/// screaming-snake vocabulary, e.g. `"NOT_FOUND"`), or, if buffering /
/// decoding the response body itself fails, a synthesized `"INTERNAL_ERROR"`.
/// Callers map `code` to a `tonic::Status` via
/// `cratestack_grpc::cool_error_code_to_tonic_code`.
pub async fn bridge_grpc_response<C, T>(
    response: Response,
    codec: &C,
    headers: &HeaderMap,
) -> Result<T, (String, String)>
where
    C: HttpTransport,
    T: DeserializeOwned,
{
    let status = response.status();
    let body_bytes = match axum::body::to_bytes(
        response.into_body(),
        cratestack_core::MAX_RESPONSE_REBUFFER_BYTES,
    )
    .await
    {
        Ok(bytes) => bytes,
        Err(error) => {
            return Err((
                "INTERNAL_ERROR".to_owned(),
                format!("failed to buffer dispatch response: {error}"),
            ));
        }
    };

    if status.is_success() {
        decode_rpc_body::<_, T>(codec, headers, &body_bytes)
            .map_err(|error| (error.code().to_owned(), error.public_message().into_owned()))
    } else {
        match decode_rpc_body::<_, CoolErrorResponse>(codec, headers, &body_bytes) {
            Ok(parsed) => Err((parsed.code, parsed.message)),
            Err(error) => Err((error.code().to_owned(), error.public_message().into_owned())),
        }
    }
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;
    use cratestack_core::CoolError;
    use serde::{Deserialize, Serialize};

    use super::*;
    use crate::transport::encode_transport_result_with_status_for;

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct Widget {
        name: String,
    }

    fn capabilities() -> cratestack_core::RouteTransportCapabilities {
        cratestack_core::RouteTransportCapabilities {
            request_types: &["application/cbor"],
            response_types: &["application/cbor"],
            default_response_type: "application/cbor",
            supports_sequence_response: false,
        }
    }

    #[tokio::test]
    async fn success_response_decodes_to_domain_value() {
        let codec = cratestack_codec_cbor::CborCodec;
        let headers = HeaderMap::new();
        let response = encode_transport_result_with_status_for(
            &codec,
            &headers,
            &capabilities(),
            StatusCode::OK,
            Ok::<_, CoolError>(Widget {
                name: "gizmo".to_owned(),
            }),
        );

        let result: Result<Widget, _> = bridge_grpc_response(response, &codec, &headers).await;
        assert_eq!(
            result.unwrap(),
            Widget {
                name: "gizmo".to_owned()
            }
        );
    }

    #[tokio::test]
    async fn error_response_decodes_to_cool_error_code_and_message() {
        let codec = cratestack_codec_cbor::CborCodec;
        let headers = HeaderMap::new();
        let response = encode_transport_result_with_status_for::<_, Widget>(
            &codec,
            &headers,
            &capabilities(),
            StatusCode::OK,
            Err(CoolError::NotFound("widget not found".to_owned())),
        );

        let result: Result<Widget, _> = bridge_grpc_response(response, &codec, &headers).await;
        let (code, message) = result.unwrap_err();
        assert_eq!(code, "NOT_FOUND");
        assert_eq!(message, "widget not found");
    }
}
