//! The cases of a signed subscription open, as seen from the generated Rust
//! client (cratestack#1378), instantiated once per transport. The refusals,
//! replay and lifetime are in `refusal_cases.rs`.

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
            pub(super) async fn through_proxy(
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

            pub(super) fn refused(
                result: Result<$crate::cose_client_stream_support::Drained, Fail>,
            ) -> bool {
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
        }
    };
}
