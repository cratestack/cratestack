//! Frame-construction tests.

#![cfg(test)]

use cratestack_core::CoolError;
use cratestack_core::rpc::RpcResponseFrame;

#[test]
fn response_frame_ok_and_err_are_mutually_exclusive() {
    let ok = RpcResponseFrame::ok(1, serde_json::json!({"x": 1}));
    assert!(ok.output.is_some());
    assert!(ok.error.is_none());

    let err = RpcResponseFrame::err(2, &CoolError::NotFound("x".into()));
    assert!(err.output.is_none());
    assert!(err.error.is_some());
}
