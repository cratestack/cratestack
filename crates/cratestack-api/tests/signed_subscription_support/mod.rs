//! Shared by `signed_subscription_{rpc,rest}.rs` (cratestack#1378): a signed
//! subscription open, sealed by hand with `ClientEnvelope::seal_call` (the
//! generated clients still refuse one locally), and the cases both
//! transports must answer alike, as a macro the two files instantiate over
//! their own generated schema.

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

/// What the registry counts: how often a handler ran.
pub type Hits = Arc<AtomicUsize>;

/// Authenticates everyone, or no one (the `@allow` refusal).
#[derive(Clone)]
pub struct Caller(pub bool);

impl cratestack::AuthProvider for Caller {
    type Error = CratestackError;

    fn authenticate(
        &self,
        _: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        core::future::ready(Ok(match self.0 {
            true => {
                CratestackContext::authenticated([("id".to_owned(), cratestack::Value::Int(1))])
            }
            false => CratestackContext::anonymous(),
        }))
    }
}

pub fn both_accepts() -> String {
    format!("{STREAM}, {SIGN1}")
}

/// The ticks a decoded cbor-seq body carried, and whether it ended cleanly.
pub fn ticks(body: &[u8]) -> (usize, bool) {
    let frames = crate::cbor_seq_support::read(body).frames;
    let nexts = frames.iter().filter(|frame| frame.next.is_some()).count();
    (
        nexts,
        frames.last().is_some_and(|frame| frame.end.is_some()),
    )
}

/// The cases both transports answer alike. `$router(allow, hits, audience)`
/// is the generated router behind the envelope layer, `$target(op, method)` a
/// [`Target`], and `$methods` the ways this transport opens one.
#[macro_export]
macro_rules! signed_subscription_cases {
    (router: $router:ident, target: $target:ident, methods: $methods:expr) => {
        mod cases {
            use std::sync::atomic::Ordering;

            use cratestack::axum::body::Body;
            use cratestack::axum::http::{Method, Request, StatusCode, header};

            use super::{$router, $target};
            use $crate::cose_support::{AUDIENCE, SIGN1, send};
            use $crate::signed_subscription_support::*;

            const MARKED: &str = "hinted";
            const UNMARKED: &str = "plain";

            #[tokio::test]
            async fn a_signed_open_streams_unsigned_frames() {
                for method in $methods {
                    let hits = Hits::default();
                    let router = $router(true, hits.clone(), AUDIENCE);
                    let sent = seal(&$target(MARKED, method), Some(&both_accepts())).await;
                    let answer = send(&router, sent.request).await;
                    assert_eq!(answer.status, StatusCode::OK, "{method}");
                    let headers = &answer.headers;
                    assert_eq!(headers["content-type"], STREAM, "{method}");
                    assert_eq!(headers["cratestack-frames"], "unsigned", "{method}");
                    assert_eq!(headers["cache-control"], "no-store", "{method}");
                    assert!(!answer.is_sealed(), "{method}");
                    assert_eq!(ticks(&answer.body), (3, true), "{method}");
                    assert_eq!(hits.load(Ordering::SeqCst), 1, "{method}");
                }
            }

            #[tokio::test]
            async fn an_unmarked_subscription_is_a_sealed_406_before_it_is_served() {
                for method in $methods {
                    let hits = Hits::default();
                    let router = $router(true, hits.clone(), AUDIENCE);
                    let sent = seal(&$target(UNMARKED, method), Some(&both_accepts())).await;
                    let answer = send(&router, sent.request).await;
                    assert_eq!(answer.status, StatusCode::NOT_ACCEPTABLE, "{method}");
                    assert!(answer.is_sealed(), "{method}");
                    assert!(
                        !answer.headers.contains_key("cratestack-frames"),
                        "{method}"
                    );
                    sent.pending
                        .open(answer.status.as_u16(), &answer.headers, answer.body)
                        .await
                        .expect("sealed under the open's binding");
                    assert_eq!(hits.load(Ordering::SeqCst), 0, "{method}: not served");
                }
            }

            #[tokio::test]
            async fn a_tampered_open_is_the_coarse_401_and_is_not_served() {
                for method in $methods {
                    let hits = Hits::default();
                    let router = $router(true, hits.clone(), AUDIENCE);
                    let target = $target(MARKED, method);
                    let mut sent = seal(&target, Some(&both_accepts())).await;
                    let mut bytes = sent.body.to_vec();
                    let last = bytes.len() - 1;
                    bytes[last] ^= 1;
                    *sent.request.body_mut() = Body::from(bytes);
                    let answer = send(&router, sent.request).await;
                    assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{method}");
                    assert!(!answer.is_sealed(), "{method}");
                    assert_eq!(hits.load(Ordering::SeqCst), 0, "{method}");
                }
            }

            #[tokio::test]
            async fn a_replayed_open_is_refused() {
                for method in $methods {
                    let hits = Hits::default();
                    let router = $router(true, hits.clone(), AUDIENCE);
                    let target = $target(MARKED, method);
                    let first = seal(&target, Some(&both_accepts())).await;
                    let replay = Request::builder()
                        .method(first.request.method().clone())
                        .uri(first.request.uri().clone())
                        .header(header::CONTENT_TYPE, SIGN1)
                        .header(header::ACCEPT, both_accepts())
                        .body(Body::from(first.body.clone()))
                        .expect("request");
                    let mut replay = replay;
                    *replay.headers_mut() = first.request.headers().clone();
                    assert_eq!(send(&router, first.request).await.status, StatusCode::OK);
                    let answer = send(&router, replay).await;
                    assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{method}");
                    assert!(!answer.is_sealed(), "{method}");
                    assert_eq!(hits.load(Ordering::SeqCst), 1, "{method}: served once");
                }
            }

            #[tokio::test]
            async fn an_open_for_another_audience_is_the_coarse_401() {
                for method in $methods {
                    let hits = Hits::default();
                    let router = $router(true, hits.clone(), "another-service");
                    let sent = seal(&$target(MARKED, method), Some(&both_accepts())).await;
                    let answer = send(&router, sent.request).await;
                    assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{method}");
                    assert_eq!(hits.load(Ordering::SeqCst), 0, "{method}");
                }
            }

            #[tokio::test]
            async fn an_unsigned_open_under_required_is_the_coarse_401() {
                for method in $methods {
                    let hits = Hits::default();
                    let router = $router(true, hits.clone(), AUDIENCE);
                    let target = $target(MARKED, method);
                    let request = Request::builder()
                        .method(Method::from_bytes(method.as_bytes()).unwrap())
                        .uri(&target.uri)
                        .header(header::ACCEPT, STREAM)
                        .body(Body::empty())
                        .unwrap();
                    let answer = send(&router, request).await;
                    assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{method}");
                    assert!(!answer.is_sealed(), "{method}");
                    assert_eq!(hits.load(Ordering::SeqCst), 0, "{method}");
                }
            }

            /// The schema's `@allow` still decides: a refusal is sealed, never a stream.
            #[tokio::test]
            async fn an_allow_refusal_is_a_sealed_403() {
                for method in $methods {
                    let hits = Hits::default();
                    let router = $router(false, hits.clone(), AUDIENCE);
                    let sent = seal(&$target(MARKED, method), Some(&both_accepts())).await;
                    let answer = send(&router, sent.request).await;
                    assert_eq!(answer.status, StatusCode::FORBIDDEN, "{method}");
                    assert!(answer.is_sealed(), "{method}");
                    assert!(
                        !answer.headers.contains_key("cratestack-frames"),
                        "{method}"
                    );
                    sent.pending
                        .open(answer.status.as_u16(), &answer.headers, answer.body)
                        .await
                        .expect("sealed");
                }
            }

            #[tokio::test]
            async fn an_open_bound_to_an_old_digest_is_the_unsigned_426() {
                for method in $methods {
                    let hits = Hits::default();
                    let router = $router(true, hits.clone(), AUDIENCE);
                    let mut target = $target(MARKED, method);
                    target.digest = [9; 32];
                    let sent = seal(&target, Some(&both_accepts())).await;
                    let answer = send(&router, sent.request).await;
                    assert_eq!(answer.status, StatusCode::UPGRADE_REQUIRED, "{method}");
                    assert!(!answer.is_sealed(), "{method}");
                    assert_eq!(hits.load(Ordering::SeqCst), 0, "{method}");
                }
            }

            /// `*/*` asks for no particular stream: a sealed open must name one.
            #[tokio::test]
            async fn a_wildcard_accept_is_a_sealed_406() {
                for method in $methods {
                    let hits = Hits::default();
                    let router = $router(true, hits.clone(), AUDIENCE);
                    let sent = seal(&$target(MARKED, method), Some("*/*")).await;
                    let answer = send(&router, sent.request).await;
                    assert_eq!(answer.status, StatusCode::NOT_ACCEPTABLE, "{method}");
                    assert!(answer.is_sealed(), "{method}");
                    assert_eq!(hits.load(Ordering::SeqCst), 0, "{method}");
                }
            }
        }
    };
}
