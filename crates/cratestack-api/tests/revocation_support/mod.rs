//! Shared by `subscription_revocation_rpc.rs` and `subscription_revocation_rest.rs`
//! (cratestack#1381): the caller, and the cases both transports must pass —
//! a live stream is ended by `revoke` with a typed reason, a reopen with the
//! same cached context is refused, and another principal is left alone.
//!
//! Each case takes a router built on the schema's `Cratestack` and that
//! `Cratestack`'s `StreamRevocations`, so it runs unchanged on REST and RPC.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cratestack::axum::Router;
use cratestack::axum::body::Body;
use cratestack::axum::http::{Request, StatusCode};
use cratestack::futures::StreamExt;
use cratestack::{CratestackContext, CratestackError, RequestContext, Value};
use cratestack_core::subscription_wire::ServerFrame;
use tower::util::ServiceExt;

#[path = "../cbor_seq_support/mod.rs"]
#[allow(dead_code)]
mod cbor_seq_support;

/// `x-sub`, `x-sid` and `x-exp` become the `sub`, `sid` and `exp` claims.
#[derive(Clone)]
pub struct Caller;

impl cratestack::AuthProvider for Caller {
    type Error = CratestackError;

    fn authenticate(
        &self,
        request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        let header = |name: &str| {
            request
                .headers
                .get(name)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned)
        };
        let mut claims = Vec::new();
        if let Some(sub) = header("x-sub") {
            claims.push(("sub".to_owned(), Value::String(sub)));
        }
        if let Some(sid) = header("x-sid") {
            claims.push(("sid".to_owned(), Value::String(sid)));
        }
        if let Some(exp) = header("x-exp").and_then(|exp| exp.parse().ok()) {
            claims.push(("exp".to_owned(), Value::Int(exp)));
        }
        core::future::ready(Ok(if claims.is_empty() {
            CratestackContext::anonymous()
        } else {
            CratestackContext::authenticated(claims)
        }))
    }
}

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

/// A subscribe request. `path` is the transport's route for the `ticks`
/// subscription procedure, with its query already on it; the headers name
/// the caller.
pub fn open(path: &str, accept: &str, sub: &str, sid: &str, exp: Option<i64>) -> Request<Body> {
    let mut request = Request::get(path)
        .header("accept", accept)
        .header("x-sub", sub)
        .header("x-sid", sid);
    if let Some(exp) = exp {
        request = request.header("x-exp", exp.to_string());
    }
    request.body(Body::empty()).unwrap()
}

pub const CBOR_SEQ: &str = "application/cbor-seq";
pub const SSE: &str = "text/event-stream";

type Chunks = std::pin::Pin<
    Box<
        dyn cratestack::futures::Stream<
                Item = Result<cratestack::axum::body::Bytes, cratestack::axum::Error>,
            > + Send,
    >,
>;

/// An open stream whose `ready` frame has been read: the server has opened it
/// and registered it for revocation.
pub struct Live {
    chunks: Chunks,
}

pub async fn opened(router: &Router, request: Request<Body>) -> Live {
    let response = router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut chunks: Chunks = Box::pin(response.into_body().into_data_stream());
    let first = tokio::time::timeout(Duration::from_secs(5), chunks.next())
        .await
        .expect("the stream opens")
        .expect("a first chunk")
        .unwrap();
    assert!(!first.is_empty(), "ready");
    Live { chunks }
}

impl Live {
    /// Everything until the server ends the stream, as bytes.
    pub async fn rest(mut self) -> Vec<u8> {
        let mut out = Vec::new();
        let drain = async {
            while let Some(chunk) = self.chunks.next().await {
                out.extend_from_slice(&chunk.unwrap());
            }
        };
        tokio::time::timeout(Duration::from_secs(5), drain)
            .await
            .expect("the server ended the stream");
        out
    }

    /// Whether the stream stays quiet for a while (not ended, not sent to).
    pub async fn stays_open(&mut self) -> bool {
        match tokio::time::timeout(Duration::from_millis(200), self.chunks.next()).await {
            Err(_) => true,
            Ok(Some(Ok(chunk))) => chunk.iter().all(|byte| *byte == 0xf6),
            Ok(_) => false,
        }
    }
}

/// The terminal `err` frame of a cbor-seq body: `(code, details.reason)`.
pub fn err_of(bytes: &[u8]) -> (String, String) {
    let body = cbor_seq_support::read(bytes);
    let frame: &ServerFrame = body.frames.last().expect("a frame");
    let err = frame.err.as_ref().expect("an err frame");
    let reason = err.details.as_ref().expect("details")["reason"]
        .as_str()
        .expect("a reason")
        .to_owned();
    (err.code.clone(), reason)
}

mod cases;

pub use cases::*;
