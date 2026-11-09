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
