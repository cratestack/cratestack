//! A signed stream is ended when its device key is revoked (cratestack#1381),
//! long before `max_signed_stream_life`, however the open arrived.

/// `$router(hits, revocations)` is the generated router behind the envelope
/// layer with `revocations` as the registry of live streams; `$target` and
/// `$methods` as in `signed_subscription_cases!`.
#[macro_export]
macro_rules! signed_subscription_revocation_cases {
    (router: $router:ident, target: $target:ident, methods: $methods:expr) => {
        mod revocation_cases {
            use cratestack::{RevocationKey, StreamEndReason, StreamRevocations};

            use super::{$router, $target};
            use $crate::cose_support::{CLIENT_PRINCIPAL, send};
            use $crate::signed_subscription_support::*;

            /// The signer's RFC 9679 thumbprint, from the principal name.
            fn signer() -> RevocationKey {
                let hex = CLIENT_PRINCIPAL.strip_prefix("cose:").unwrap();
                let bytes: Vec<u8> = (0..hex.len())
                    .step_by(2)
                    .map(|at| u8::from_str_radix(&hex[at..at + 2], 16).unwrap())
                    .collect();
                RevocationKey::Signer(bytes.try_into().unwrap())
            }

            /// Revoking the device key that signed the open ends the stream
            /// with `permission_denied` / `device_revoked`, with the clock
            /// two minutes into a one-hour lifetime. Paused time.
            #[tokio::test(start_paused = true)]
            async fn revoking_the_signing_device_ends_a_signed_stream() {
                for method in $methods {
                    let revocations = StreamRevocations::default();
                    let router = $router(Hits::default(), revocations.clone());
                    let sent =
                        seal(&endless($target("hinted", method)), Some(&both_accepts())).await;
                    let started = tokio::time::Instant::now();
                    let answer = tokio::spawn(async move { send(&router, sent.request).await });

                    tokio::time::sleep(std::time::Duration::from_secs(120)).await;
                    let ended = revocations.revoke(signer(), StreamEndReason::DeviceRevoked, None);
                    assert_eq!(ended, 1, "{method}: the signed stream was watching");

                    let answer = answer.await.unwrap();
                    assert!(
                        started.elapsed() < std::time::Duration::from_secs(600),
                        "{method}: ended long before the one-hour lifetime"
                    );
                    let frames = $crate::cbor_seq_support::read(&answer.body).frames;
                    let err = frames
                        .last()
                        .unwrap()
                        .err
                        .as_ref()
                        .expect("a terminal err frame");
                    assert_eq!(
                        (err.code.as_str(), err.message.as_str()),
                        ("permission_denied", "device revoked"),
                        "{method}"
                    );
                    assert_eq!(
                        err.details.as_ref().unwrap()["reason"],
                        "device_revoked",
                        "{method}"
                    );
                }
            }
        }
    };
}
