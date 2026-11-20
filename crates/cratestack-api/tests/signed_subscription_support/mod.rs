//! Shared by `signed_subscription_{rpc,rest}.rs` (cratestack#1378): a signed
//! subscription open, sealed by hand with `ClientEnvelope::seal_call` (the
//! generated clients still refuse one locally), and the cases both
//! transports must answer alike, as macros the two files instantiate over
//! their own generated schema (`fixtures`, `cases`, `stream_cases`).

// Each test binary uses a different subset of these helpers.
#![allow(dead_code, unused_imports)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use cratestack::axum::body::Body;
use cratestack::axum::body::Bytes;
use cratestack::axum::http::{HeaderMap, Method, Request, StatusCode, header};
use cratestack::{CratestackContext, CratestackError, RequestContext};
use cratestack_client_rust::{PendingResponse, RouteRef, SealCall};

use crate::cose_support::{self, Answer, SIGN1, send};

pub const STREAM: &str = "application/cbor-seq";
pub const ARGS: &str = "args=%7B%22to%22%3A3%7D";
/// The open's `to` that makes the fixture registry stream without end.
pub const ENDLESS_ARGS: &str = "args=%7B%22to%22%3A-1%7D";

/// Where and how one open is sent.
pub struct Target {
    pub method: &'static str,
    /// The bound route: `subscribe/<op id>` on RPC, the path template on REST.
    pub route: String,
    pub uri: String,
    pub query: Option<&'static str>,
    /// The op contract digest the open is bound under.
    pub digest: [u8; 32],
    /// The open's payload; empty for a `GET`.
    pub payload: Vec<u8>,
}

/// `target`, asking for a stream that never ends (`to: -1`).
pub fn endless(mut target: Target) -> Target {
    target.uri = target.uri.replace(ARGS, ENDLESS_ARGS);
    target.query = target.query.map(|_| ENDLESS_ARGS);
    if !target.payload.is_empty() {
        let open = serde_json::json!({ "args": { "to": -1 } });
        target.payload =
            cratestack::CratestackCodec::encode(&cratestack_codec_cbor::CborCodec, &open)
                .expect("encode");
    }
    target
}

mod cases;
mod fixtures;
mod revocation_cases;
mod stream_cases;

pub use fixtures::*;

/// A sealed open, ready to send, and what opens its answer.
pub struct Sent {
    pub request: Request<Body>,
    pub body: Bytes,
    pub pending: PendingResponse,
}

/// Seal `target`. `accept` replaces the `Accept` the envelope sets.
pub async fn seal(target: &Target, accept: Option<&str>) -> Sent {
    let route = match target.route.strip_prefix("subscribe/") {
        Some(op) => RouteRef::rpc(Box::leak(format!("subscribe/{op}").into_boxed_str())),
        None => RouteRef::new(Box::leak(target.route.clone().into_boxed_str()), &[]),
    };
    let mut call = SealCall::new(target.method, route, target.digest).query(target.query);
    if !target.payload.is_empty() {
        call = call.payload(&target.payload, "application/cbor");
    }
    let sealed = cose_support::client_envelope()
        .seal_call(call)
        .await
        .expect("seal the open");
    let mut builder = Request::builder()
        .method(Method::from_bytes(target.method.as_bytes()).expect("method"))
        .uri(&target.uri);
    for (name, value) in &sealed.headers {
        if name != header::ACCEPT || accept.is_none() {
            builder = builder.header(name, value);
        }
    }
    if let Some(accept) = accept {
        builder = builder.header(header::ACCEPT, accept);
    }
    Sent {
        request: builder
            .body(Body::from(sealed.body.clone()))
            .expect("request"),
        body: sealed.body,
        pending: sealed.pending,
    }
}
