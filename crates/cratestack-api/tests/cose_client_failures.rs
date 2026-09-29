//! What a signing client must refuse (cratestack#1007, ADR 0006 §4): a
//! response that is not the answer to *this* request, a downgrade to plain,
//! and a server that would not sign. Every case runs over REST and RPC, in
//! every mode, with a proxy between the generated client and the generated
//! server that does the tampering.

mod cose_client_support;
#[path = "cose_client_support/rest_app.rs"]
mod rest_app;
#[path = "cose_client_support/rpc_app.rs"]
mod rpc_app;

use cose_client_support::{
    AUDIENCE, KINDS, Kind, Outcome, Proxy, Tamper, client_envelope, proxy, runtime,
    runtime_following_redirects,
};
use cratestack::CratestackCodec;
use cratestack_client_rust::{CborCodec, CratestackClient, EnvelopeError};

fn forged_reply() -> Vec<u8> {
    cratestack_codec_cbor::CborCodec
        .encode(&serde_json::json!({ "echo": "forged" }))
        .expect("encode")
}

/// The suite, once per transport: `$app` is that transport's test app.
macro_rules! failure_suite {
    ($app:ident) => {
        use super::*;

        /// The generated server, a proxy in front of it, and a client that
        /// talks to the proxy.
        async fn through_proxy(kind: Kind, audience: &'static str) -> (Proxy, $app::Client) {
            let upstream = $app::server(kind).await;
            let proxy = proxy(upstream).await;
            let client = $app::Client::new(runtime(proxy.addr, client_envelope(kind, audience)));
            (proxy, client)
        }

        // 2. A genuine, validly signed answer, replayed for another request.
        #[tokio::test]
        async fn a_response_sealed_for_another_request_is_rejected() {
            for kind in KINDS {
                let (proxy, client) = through_proxy(kind, AUDIENCE).await;
                assert!(matches!(
                    $app::ping(&client, "a", &[]).await,
                    Outcome::Ok(_)
                ));
                proxy.tamper(Tamper::ReplayPrevious);
                assert_eq!(
                    $app::ping(&client, "b", &[]).await,
                    Outcome::Unverified,
                    "{kind:?}"
                );
            }
        }

        // 3. The proxy changes only the status: 200 -> 201.
        #[tokio::test]
        async fn a_response_whose_status_was_rewritten_is_rejected() {
            for kind in KINDS {
                let (proxy, client) = through_proxy(kind, AUDIENCE).await;
                proxy.tamper(Tamper::Status(201.try_into().unwrap()));
                assert_eq!(
                    $app::ping(&client, "a", &[]).await,
                    Outcome::Unverified,
                    "{kind:?}"
                );
            }
        }

        // 4. No downgrade: a plain answer is never decoded, even a valid one.
        #[tokio::test]
        async fn a_plain_answer_is_an_error_not_a_reply() {
            for kind in KINDS {
                let (proxy, client) = through_proxy(kind, AUDIENCE).await;
                proxy.tamper(Tamper::Plain(forged_reply()));
                assert_eq!(
                    $app::ping(&client, "a", &[]).await,
                    Outcome::Unsigned(200),
                    "{kind:?}: a stripped seal must not turn into a plain 200"
                );
            }
        }

        // 5. The layer's own refusals are unsigned: an error, body unread.
        #[tokio::test]
        async fn an_unsigned_refusal_from_the_layer_is_an_error() {
            for kind in KINDS {
                // A client addressing some other service: the server's
                // audience differs, so the layer answers 401, unsigned.
                let (_proxy, client) = through_proxy(kind, "someone-else").await;
                assert_eq!(
                    $app::ping(&client, "a", &[]).await,
                    Outcome::Unsigned(401),
                    "{kind:?}"
                );
            }
        }

        // The client really seals: nothing plain reaches the wire.
        #[tokio::test]
        async fn every_request_on_the_wire_is_sealed() {
            for kind in KINDS {
                let (proxy, client) = through_proxy(kind, AUDIENCE).await;
                assert!(matches!(
                    $app::ping(&client, "a", &[]).await,
                    Outcome::Ok(_)
                ));
                let types = proxy.request_content_types();
                assert!(
                    types.iter().all(|t| t
                        .as_deref()
                        .is_some_and(|t| t.starts_with("application/cose"))),
                    "{kind:?}: {types:?}"
                );
            }
        }

        // A redirect is never followed: a `303` would turn the sealed POST
        // into a plain authenticated GET elsewhere, a `307` would re-send the
        // sealed bytes to another `Location`.
        #[tokio::test]
        async fn a_redirect_is_not_followed() {
            for kind in KINDS {
                for code in [303, 307] {
                    let (proxy, client) = through_proxy(kind, AUDIENCE).await;
                    proxy.tamper(Tamper::Redirect(code));
                    assert_eq!(
                        $app::ping(&client, "a", &[]).await,
                        Outcome::Unsigned(code),
                        "{kind:?} {code}"
                    );
                    assert_eq!(
                        proxy.request_content_types().len(),
                        1,
                        "{kind:?} {code}: the Location was requested"
                    );
                }
            }
        }

        // A caller-supplied client that does follow redirects is caught after
        // the fact: the answer did not come from the URL that was sealed for.
        #[tokio::test]
        async fn an_answer_from_a_redirected_url_is_rejected() {
            for kind in KINDS {
                for code in [303, 307] {
                    let upstream = $app::server(kind).await;
                    let proxy = proxy(upstream).await;
                    let client = $app::Client::new(runtime_following_redirects(
                        proxy.addr,
                        client_envelope(kind, AUDIENCE),
                    ));
                    proxy.tamper(Tamper::Redirect(code));
                    assert_eq!(
                        $app::ping(&client, "a", &[]).await,
                        Outcome::Unverified,
                        "{kind:?} {code}"
                    );
                }
            }
        }

        // 7. Streams are refused locally, before anything is sent.
        #[tokio::test]
        async fn a_stream_is_refused_locally() {
            for kind in KINDS {
                let (proxy, client) = through_proxy(kind, AUDIENCE).await;
                assert_eq!($app::stream(&client).await, Outcome::StreamsUnsupported);
                assert!(proxy.request_content_types().is_empty(), "nothing was sent");
            }
        }
    };
}

mod rest {
    failure_suite!(rest_app);

    // 6. `Idempotency-Key` is bound: a proxy that strips it changes what the
    // server verifies. (RPC calls carry no per-call headers.)
    #[tokio::test]
    async fn a_stripped_idempotency_key_fails_verification() {
        for kind in KINDS {
            let (proxy, client) = through_proxy(kind, AUDIENCE).await;
            let keyed = [("Idempotency-Key", "k-1")];
            assert!(matches!(
                rest_app::ping(&client, "a", &keyed).await,
                Outcome::Ok(_)
            ));
            proxy.tamper(Tamper::DropRequestHeader("idempotency-key"));
            assert_eq!(
                rest_app::ping(&client, "b", &keyed).await,
                Outcome::Unsigned(401),
                "{kind:?}"
            );
        }
    }

    // `If-Match` is bound the same way.
    #[tokio::test]
    async fn a_stripped_if_match_fails_verification() {
        for kind in KINDS {
            let (proxy, client) = through_proxy(kind, AUDIENCE).await;
            let matched = [("If-Match", "\"v1\"")];
            assert!(matches!(
                rest_app::ping(&client, "a", &matched).await,
                Outcome::Ok(_)
            ));
            proxy.tamper(Tamper::DropRequestHeader("if-match"));
            assert_eq!(
                rest_app::ping(&client, "b", &matched).await,
                Outcome::Unsigned(401),
                "{kind:?}"
            );
        }
    }

    // Two values, or one with padding, cannot be sealed faithfully: refused
    // before anything is sent.
    #[tokio::test]
    async fn an_unsealable_bound_header_is_refused_locally() {
        let (proxy, client) = through_proxy(Kind::Ed25519, AUDIENCE).await;
        for headers in [
            &[("Idempotency-Key", "a"), ("Idempotency-Key", "b")][..],
            &[("Idempotency-Key", " padded")][..],
            &[("If-Match", "\"v1\" ")][..],
        ] {
            let outcome = rest_app::ping(&client, "a", headers).await;
            assert!(
                matches!(&outcome, Outcome::Other(text) if text.contains("cannot be sealed")),
                "{headers:?}: {outcome:?}"
            );
        }
        assert!(proxy.request_content_types().is_empty(), "nothing was sent");
    }
}

mod rpc {
    failure_suite!(rpc_app);
}

#[tokio::test]
async fn a_json_client_cannot_take_an_envelope() {
    let base = reqwest::Url::parse("http://127.0.0.1:1").unwrap();
    let client = CratestackClient::new(
        cratestack_client_rust::ClientConfig::new(base),
        cratestack_client_rust::JsonCodec,
    );
    let error = client
        .with_envelope(client_envelope(Kind::Ed25519, AUDIENCE))
        .err()
        .expect("an envelope wraps CBOR");
    assert!(
        matches!(error, cratestack_client_rust::ClientError::BadInput(_)),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_bare_client_without_a_route_or_digest_says_what_is_missing() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let base = reqwest::Url::parse("http://127.0.0.1:1").unwrap();
    let client = CratestackClient::new(cratestack_client_rust::ClientConfig::new(base), CborCodec)
        .with_envelope(client_envelope(Kind::Ed25519, AUDIENCE))
        .expect("CBOR");
    let error = client
        .get::<serde_json::Value>("/widgets", &[], &[])
        .await
        .expect_err("no route");
    assert!(
        matches!(error, cratestack_client_rust::ClientError::BadInput(_)),
        "{error:?}"
    );
    let _ = EnvelopeError::Unverified;
}
