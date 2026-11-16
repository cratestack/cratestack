//! The cases of a signed subscription open, as seen from the generated Rust
//! client (cratestack#1378), instantiated once per transport.

/// `$app` is the transport's test app (`rpc_app`, `rest_app`).
#[macro_export]
macro_rules! client_stream_cases {
    ($app:ident) => {
        mod stream_cases {
            use std::sync::atomic::Ordering;
            use std::time::Duration;

            use cratestack_client_rust::{EnvelopeError, SubscriptionTransport};

            use super::$app;
            use $crate::cose_client_stream_support::Fail;
            use $crate::cose_client_support::{Tamper, proxy};

            const LIFE: Duration = Duration::from_secs(3600);

            /// A client of the generated server behind a tampering proxy.
            async fn through_proxy(
                transport: SubscriptionTransport,
            ) -> (
                $crate::cose_client_support::Proxy,
                $app::Client,
                std::net::SocketAddr,
                $crate::cose_client_stream_support::Hits,
            ) {
                // `reqwest` ships no crypto provider: install one before the
                // proxy builds its client.
                let _ = rustls::crypto::ring::default_provider().install_default();
                let (upstream, hits) = $app::server(true, LIFE).await;
                let proxy = proxy(upstream).await;
                let client = $app::client(proxy.addr, transport);
                (proxy, client, upstream, hits)
            }

            fn refused(result: Result<$crate::cose_client_stream_support::Drained, Fail>) -> bool {
                matches!(
                    result,
                    Err(Fail::Envelope(EnvelopeError::UnsignedFramesRefused))
                )
            }

            /// Every binding a signing client can open: `Auto` (cbor-seq),
            /// cbor-seq, and SSE. The open is a sealed COSE message, the
            /// stream plain.
            #[tokio::test]
            async fn a_marked_subscription_streams_after_a_sealed_open() {
                for transport in [
                    SubscriptionTransport::Auto,
                    SubscriptionTransport::CborSeq,
                    SubscriptionTransport::Sse,
                ] {
                    let (proxy, client, _, hits) = through_proxy(transport).await;
                    let drained = $app::hinted_ticks(&client, 3).await.expect("an open");
                    assert_eq!(drained.ticks, [3, 2, 1], "{transport:?}");
                    assert!(
                        drained.ended.is_none(),
                        "{transport:?}: {:?}",
                        drained.ended
                    );
                    assert_eq!(hits.load(Ordering::SeqCst), 1, "{transport:?}");
                    let sent = proxy.captured();
                    assert_eq!(sent.len(), 1, "{transport:?}");
                    let header = |name: &str| {
                        sent[0]
                            .headers
                            .iter()
                            .find(|(key, _)| key == name)
                            .map(|(_, value)| value.as_str())
                    };
                    // The seal's own media type, and the stream beside it.
                    let cose = header("content-type").expect("a content type");
                    assert!(cose.starts_with("application/cose"), "{cose}");
                    let want = match transport {
                        SubscriptionTransport::Sse => format!("text/event-stream, {cose}"),
                        _ => format!("application/cbor-seq, {cose}"),
                    };
                    assert_eq!(header("accept"), Some(want.as_str()), "{transport:?}");
                }
            }

            /// A proxy that strips the marker turns a stream into a body no
            /// one vouches for.
            #[tokio::test]
            async fn a_stream_that_lost_its_marker_is_refused() {
                for transport in [SubscriptionTransport::CborSeq, SubscriptionTransport::Sse] {
                    let (proxy, client, ..) = through_proxy(transport).await;
                    proxy.tamper(Tamper::StripFrames);
                    assert!(
                        refused($app::hinted_ticks(&client, 3).await),
                        "{transport:?}"
                    );
                }
            }

            /// A plain `200` in the stream's place is anything a proxy made
            /// up: it is never read as frames.
            #[tokio::test]
            async fn a_forged_plain_200_is_refused() {
                let (proxy, client, ..) = through_proxy(SubscriptionTransport::CborSeq).await;
                proxy.tamper(Tamper::Plain(vec![0xa0]));
                assert!(refused($app::hinted_ticks(&client, 3).await));
            }

            /// The marker is not enough: the body must be the type asked for.
            #[tokio::test]
            async fn a_marked_answer_of_another_type_is_refused() {
                for (transport, served) in [
                    (SubscriptionTransport::CborSeq, "text/event-stream"),
                    (SubscriptionTransport::CborSeq, "application/cbor"),
                    (SubscriptionTransport::Sse, "application/cbor-seq"),
                ] {
                    let (proxy, client, ..) = through_proxy(transport).await;
                    proxy.tamper(Tamper::ContentType(served));
                    assert!(
                        refused($app::hinted_ticks(&client, 3).await),
                        "{transport:?} answered {served}"
                    );
                }
            }

            /// An unsigned refusal, whatever its status, is `Unsigned`.
            #[tokio::test]
            async fn an_unsigned_error_is_unsigned_not_a_remote_error() {
                let (proxy, client, ..) = through_proxy(SubscriptionTransport::CborSeq).await;
                proxy.tamper(Tamper::Status(500.try_into().unwrap()));
                assert!(matches!(
                    $app::hinted_ticks(&client, 3).await,
                    Err(Fail::Envelope(EnvelopeError::Unsigned { status: 500 }))
                ));
            }

            /// Only a subscription the schema marks can be opened signed:
            /// the rest is refused before a request is made.
            #[tokio::test]
            async fn an_unmarked_subscription_is_refused_with_nothing_sent() {
                for transport in [SubscriptionTransport::Auto, SubscriptionTransport::Sse] {
                    let (proxy, client, _, hits) = through_proxy(transport).await;
                    assert!(
                        matches!(
                            $app::plain_ticks(&client, 3).await,
                            Err(Fail::Envelope(EnvelopeError::StreamsUnsupported))
                        ),
                        "{transport:?}"
                    );
                    assert!(proxy.captured().is_empty(), "{transport:?}: sent");
                    assert_eq!(hits.load(Ordering::SeqCst), 0);
                }
            }

            /// A socket has no sealed open, and `Auto` never falls back to one.
            #[tokio::test]
            async fn a_socket_pin_is_refused_with_nothing_sent() {
                for transport in [
                    SubscriptionTransport::WebSocket,
                    SubscriptionTransport::WebTransport,
                ] {
                    let (proxy, client, ..) = through_proxy(transport).await;
                    assert!(
                        matches!(
                            $app::hinted_ticks(&client, 3).await,
                            Err(Fail::Envelope(EnvelopeError::StreamsUnsupported))
                        ),
                        "{transport:?}"
                    );
                    assert!(proxy.captured().is_empty(), "{transport:?}: sent");
                }
            }

            /// A policy refusal is sealed, and is opened into the error it is.
            #[tokio::test]
            async fn an_allow_refusal_is_a_remote_403() {
                let (upstream, hits) = $app::server(false, LIFE).await;
                let client = $app::client(upstream, SubscriptionTransport::CborSeq);
                match $app::hinted_ticks(&client, 3).await {
                    Err(Fail::Remote { status: 403, .. }) => {}
                    other => panic!("expected a remote 403, got {other:?}"),
                }
                assert_eq!(hits.load(Ordering::SeqCst), 0, "the handler ran");
            }

            /// The captured bytes of an open, sent again, are a replay the
            /// server refuses; opening again is a new open with a fresh nonce.
            #[tokio::test]
            async fn a_replayed_open_is_refused_and_a_resubscribe_is_not_one() {
                let (proxy, client, upstream, hits) =
                    through_proxy(SubscriptionTransport::CborSeq).await;
                let first = $app::hinted_ticks(&client, 3).await.expect("an open");
                assert_eq!(first.ticks, [3, 2, 1]);
                let open = proxy.captured().remove(0);
                let mut request = reqwest::Client::new().request(
                    open.method.parse().unwrap(),
                    format!("http://{upstream}{}", open.path_and_query),
                );
                for (name, value) in &open.headers {
                    if !["host", "content-length"].contains(&name.as_str()) {
                        request = request.header(name, value);
                    }
                }
                let replay = request.body(open.body.clone()).send().await.unwrap();
                assert_eq!(replay.status(), 401, "the replay was served");
                assert_eq!(hits.load(Ordering::SeqCst), 1, "the replay ran the handler");

                let second = $app::hinted_ticks(&client, 3).await.expect("an open");
                assert_eq!(second.ticks, [3, 2, 1]);
                let sent = proxy.captured();
                assert_eq!(sent.len(), 2);
                assert_ne!(sent[0].body, sent[1].body, "the nonce was reused");
                assert_eq!(hits.load(Ordering::SeqCst), 2);
            }

            /// The server ends a signed stream at its lifetime with a terminal
            /// `unavailable`, which the client reads as the end of the stream.
            #[tokio::test]
            async fn a_stream_ends_at_the_lifetime_with_a_remote_unavailable() {
                let (upstream, _) = $app::server(true, Duration::from_secs(1)).await;
                let client = $app::client(upstream, SubscriptionTransport::CborSeq);
                let drained =
                    tokio::time::timeout(Duration::from_secs(30), $app::hinted_ticks(&client, -1))
                        .await
                        .expect("the stream ended on its own")
                        .expect("an open");
                assert!(!drained.ticks.is_empty(), "items flowed before the cut");
                match drained.ended {
                    Some(Fail::Remote {
                        status: 503,
                        message,
                    }) => {
                        assert_eq!(message, "stream lifetime")
                    }
                    other => panic!("expected a remote 503, got {other:?}"),
                }
            }
        }
    };
}
