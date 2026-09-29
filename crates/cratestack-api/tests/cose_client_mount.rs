//! A router mounted under a prefix (cratestack#1007): the seal binds the
//! mount's own path parameters ahead of the route's, so a request signed for
//! one tenant cannot be replayed at another. `with_mount_params` is how the
//! client names them; a plain prefix such as `/api` needs none.

mod cose_client_support;
#[path = "cose_client_support/rest_app.rs"]
mod rest_app;
#[path = "cose_client_support/rpc_app.rs"]
mod rpc_app;

use cose_client_support::{AUDIENCE, KINDS, Outcome, client_envelope, runtime_at};

macro_rules! mount_suite {
    ($app:ident) => {
        use super::*;

        fn client(
            addr: std::net::SocketAddr,
            base: &str,
            params: &[&str],
            kind: cose_client_support::Kind,
        ) -> $app::Client {
            let envelope = client_envelope(kind, AUDIENCE)
                .with_mount_params(params.iter().map(|p| (*p).to_owned()).collect());
            $app::Client::new(runtime_at(addr, base, envelope))
        }

        #[tokio::test]
        async fn the_mount_params_the_server_sees_are_sealed() {
            for kind in KINDS {
                let addr = $app::server_mounted(kind, "/t/{tenant}").await;
                let client = client(addr, "/t/acme", &["acme"], kind);
                let outcome = $app::ping(&client, "a", &[]).await;
                assert!(matches!(outcome, Outcome::Ok(_)), "{kind:?}: {outcome:?}");
            }
        }

        #[tokio::test]
        async fn other_or_missing_mount_params_fail_verification() {
            for kind in KINDS {
                let addr = $app::server_mounted(kind, "/t/{tenant}").await;
                for params in [&["other"][..], &[][..]] {
                    let client = client(addr, "/t/acme", params, kind);
                    assert_eq!(
                        $app::ping(&client, "a", &[]).await,
                        Outcome::Unsigned(401),
                        "{kind:?} {params:?}"
                    );
                }
            }
        }

        #[tokio::test]
        async fn a_plain_prefix_needs_no_mount_params() {
            for kind in KINDS {
                let addr = $app::server_mounted(kind, "/api").await;
                let client = client(addr, "/api", &[], kind);
                assert!(
                    matches!($app::ping(&client, "a", &[]).await, Outcome::Ok(_)),
                    "{kind:?}"
                );
            }
        }
    };
}

mod rest {
    mount_suite!(rest_app);
}

mod rpc {
    mount_suite!(rpc_app);
}
