//! A middleware refusal whose code no `CratestackError` variant names
//! (the envelope layer's `426 contract_unsupported`, cratestack#1123).
//!
//! The RPC binding's code vocabulary is gRPC-style lowercase and REST's is
//! screaming snake, so the two are given separately; the body shapes are
//! the same two every other middleware refusal uses.

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use cratestack_core::CratestackErrorResponse;
use cratestack_core::rpc::RpcErrorBody;

use super::{encode_with_status, is_rpc_path, middleware_codec};

/// Encode `message` as the error envelope this request's transport expects,
/// at `status`, with `rpc_code` on an RPC path and `rest_code` otherwise.
pub(crate) fn middleware_coded_response(
    headers: &HeaderMap,
    path: &str,
    status: StatusCode,
    (rpc_code, rest_code): (&str, &str),
    message: &str,
) -> Response {
    let codec = middleware_codec();
    if is_rpc_path(path) {
        let body = RpcErrorBody {
            code: rpc_code.to_owned(),
            message: message.to_owned(),
            details: None,
        };
        encode_with_status(&codec, headers, status, &body)
    } else {
        let body = CratestackErrorResponse {
            code: rest_code.to_owned(),
            message: message.to_owned(),
            details: None,
        };
        encode_with_status(&codec, headers, status, &body)
    }
}
