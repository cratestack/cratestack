//! The stream-shaped cases both transports answer alike (cratestack#1378):
//! how long a signed stream lives, and what an open must ask for.

/// `$router(allow, hits, audience)` and `$router_life(allow, hits, audience,
/// life)` are the generated router behind the envelope layer, without and with
/// a configured `max_signed_stream_life`; `$target` and `$methods` as in
/// `signed_subscription_cases!`.
#[macro_export]
macro_rules! signed_subscription_stream_cases {
    (router: $router:ident, router_life: $router_life:ident, target: $target:ident, methods: $methods:expr) => {
        mod stream_cases {
            use std::sync::atomic::Ordering;

            use cratestack::axum::http::StatusCode;

            use super::{$router, $router_life, $target};
            use $crate::cose_support::{AUDIENCE, send};
            use $crate::signed_subscription_support::*;

            const MARKED: &str = "hinted";

            /// The deadline the layer inserts reaches the generated handler's
            /// feed: a stream that never ends is cut at `max_signed_stream_life`
            /// with a terminal `unavailable` / "stream lifetime" frame, however
            /// the open arrived. Paused time, so the life costs no wall clock.
            #[tokio::test(start_paused = true)]
            async fn a_signed_stream_ends_at_the_lifetime_with_an_err_frame() {
                for method in $methods {
                    let hits = Hits::default();
                    let life = std::time::Duration::from_secs(5);
                    let router = $router_life(true, hits.clone(), AUDIENCE, life);
                    let target = endless($target(MARKED, method));
                    let sent = seal(&target, Some(&both_accepts())).await;
                    let started = tokio::time::Instant::now();
                    let answer = tokio::time::timeout(
                        std::time::Duration::from_secs(3600),
                        send(&router, sent.request),
                    )
                    .await
                    .expect("the stream ended on its own");
                    // The configured 5 s, not the one-hour default, was applied.
                    let lived = started.elapsed();
                    assert!(
                        lived >= life && lived < life + std::time::Duration::from_secs(1),
                        "{method}: the stream lived {lived:?}, not the configured {life:?}"
                    );
                    assert_eq!(answer.status, StatusCode::OK, "{method}");
                    assert_eq!(answer.headers["cratestack-frames"], "unsigned", "{method}");
                    assert!(!answer.is_sealed(), "{method}");
                    let frames = $crate::cbor_seq_support::read(&answer.body).frames;
                    let last = frames.last().expect("frames");
                    let err = last.err.as_ref().expect("a terminal err frame");
                    assert_eq!(
                        (err.code.as_str(), err.message.as_str()),
                        ("unavailable", "stream lifetime"),
                        "{method}"
                    );
                    let nexts = frames.iter().filter(|frame| frame.next.is_some()).count();
                    assert!(nexts > 0, "{method}: items flowed before the cut");
                    assert_eq!(hits.load(Ordering::SeqCst), 1, "{method}");
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
