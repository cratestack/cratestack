//! The refusals, the replay and the lifetime of a signed subscription open,
//! as seen from the generated Rust client (cratestack#1378), instantiated once
//! per transport after `cases.rs`, whose helpers they share.

/// `$app` is the transport's test app (`rpc_app`, `rest_app`).
#[macro_export]
macro_rules! client_stream_refusal_cases {
    ($app:ident) => {
        mod stream_refusal_cases {
            use std::sync::atomic::Ordering;
            use std::time::Duration;

            use cratestack_client_rust::{EnvelopeError, SubscriptionTransport};

            use super::$app;
            use super::stream_cases::through_proxy;
            use $crate::cose_client_stream_support::Fail;
            use $crate::cose_client_support::Tamper;

            const LIFE: Duration = Duration::from_secs(3600);

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

            /// A redirect is never followed: the seal binds one route, and a hop
            /// would send the signed open somewhere it was not signed for.
            #[tokio::test]
            async fn a_redirect_is_not_followed() {
                for code in [303, 307] {
                    let (proxy, client, ..) = through_proxy(SubscriptionTransport::CborSeq).await;
                    proxy.tamper(Tamper::Redirect(code));
                    assert!(
                        matches!(
                            $app::hinted_ticks(&client, 3).await,
                            Err(Fail::Envelope(EnvelopeError::Unsigned { status })) if status == code
                        ),
                        "{code}"
                    );
                    assert_eq!(proxy.captured().len(), 1, "{code}: the Location was requested");
                }
            }

            /// The unsigned `426` "this op's shape is not served" is a hint
            /// that names the op, as on a unary call.
            #[tokio::test]
            async fn an_unsigned_426_contract_refusal_is_contract_unsupported() {
                // A CBOR map `{"code": "contract_unsupported"}`.
                let code = "contract_unsupported";
                let mut body = vec![0xa1, 0x64, b'c', b'o', b'd', b'e', 0x60 + code.len() as u8];
                body.extend_from_slice(code.as_bytes());
                for transport in [SubscriptionTransport::CborSeq, SubscriptionTransport::Sse] {
                    let (proxy, client, ..) = through_proxy(transport).await;
                    proxy.tamper(Tamper::Answer(426.try_into().unwrap(), body.clone()));
                    assert!(
                        matches!(
                            $app::hinted_ticks(&client, 3).await,
                            Err(Fail::Envelope(EnvelopeError::ContractUnsupported { .. }))
                        ),
                        "{transport:?}"
                    );
                }
                // A `426` that does not say so stays a plain unsigned refusal.
                let (proxy, client, ..) = through_proxy(SubscriptionTransport::CborSeq).await;
                proxy.tamper(Tamper::Answer(426.try_into().unwrap(), vec![0xa0]));
                assert!(matches!(
                    $app::hinted_ticks(&client, 3).await,
                    Err(Fail::Envelope(EnvelopeError::Unsigned { status: 426 }))
                ));
            }
        }
    };
}
