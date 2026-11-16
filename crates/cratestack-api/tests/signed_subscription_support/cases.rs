//! The cases both transports answer alike (cratestack#1378).

/// `$router(allow, hits, audience)` is the generated router behind the
/// envelope layer, `$target(op, method)` a [`Target`](super::Target), and
/// `$methods` the ways this transport opens one.
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
        }
    };
}
