//! What every envelope-layer test shares, with or without COSE: the
//! constants, plain requests, and reading an answer. The COSE client side
//! is `cose_support`, re-exported here under the `cose` feature.

// Shared with the COSE suites, which the `envelope` feature alone does
// not compile; what only they use is dead there.
#![cfg_attr(not(feature = "cose"), allow(dead_code))]

use axum::Router;
use axum::body::Body;
use axum::extract::Request;
use axum::response::Response;
use bytes::Bytes;
use http::{HeaderMap, Method, StatusCode, header};
use tower::ServiceExt;

#[cfg(feature = "cose")]
pub use super::cose_support::*;

pub const AUDIENCE: &str = "payments";
pub const SCHEMA: [u8; 32] = [7; 32];
pub const SIGN1: &str = "application/cose; cose-type=\"cose-sign1\"";
/// `{"a": 1}` in CBOR.
pub const PAYLOAD: &[u8] = &[0xa1, 0x61, 0x61, 0x01];

pub fn plain_request(method: Method, uri: &str, body: &'static [u8]) -> Request {
    http::Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/cbor")
        .body(Body::from(body))
        .expect("request")
}

pub struct Answer {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Bytes,
}

impl Answer {
    pub fn content_type(&self) -> &str {
        self.headers
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
    }

    pub fn seen(&self, name: &str) -> &str {
        self.headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("<absent>")
    }

    pub fn is_sealed(&self) -> bool {
        self.content_type() == SIGN1
    }
}

pub async fn send(router: &Router, req: Request) -> Answer {
    let response: Response = router.clone().oneshot(req).await.expect("infallible");
    let (parts, body) = response.into_parts();
    let body = axum::body::to_bytes(body, usize::MAX).await.expect("body");
    Answer {
        status: parts.status,
        headers: parts.headers,
        body,
    }
}

/// A `/rpc/batch` body (CBOR) with one frame per op.
pub fn batch_frames(ops: &[&str]) -> Vec<u8> {
    let frames: Vec<serde_json::Value> = ops
        .iter()
        .enumerate()
        .map(|(id, op)| serde_json::json!({ "id": id, "op": op, "input": {} }))
        .collect();
    minicbor_serde::to_vec(&frames).expect("frames")
}

/// The error body's `code`, decoded from CBOR.
pub fn error_code(body: &[u8]) -> String {
    let value: serde_json::Value = minicbor_serde::from_slice(body).expect("a CBOR error body");
    value["code"].as_str().expect("code").to_owned()
}
