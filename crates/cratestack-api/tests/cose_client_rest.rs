//! The generated REST client seals and opens (cratestack#1007), against the
//! real generated server with the envelope layer in `Required` mode, on a
//! real listener. The RPC twin is `cose_client_rpc.rs`, and the two must
//! stay in step (transport parity); the failure paths of both are in
//! `cose_client_failures.rs`.

mod cose_client_support;
#[path = "cose_client_support/rest_app.rs"]
mod rest_app;

use cose_client_support::{KINDS, alg_of};
use rest_app::{args, client, client_for, server};

#[tokio::test]
async fn a_signed_versioned_procedure_round_trips_in_every_mode() {
    for kind in KINDS {
        let client = client_for(server(kind).await, kind);
        let reply = client
            .procedures()
            .ping(&args("hello"), &[])
            .await
            .unwrap_or_else(|error| panic!("{kind:?}: {error}"));
        // The handler saw the opened payload, and the envelope's signer.
        assert_eq!(
            reply.echo,
            format!("v2:hello|signer=Some({})", alg_of(kind)),
            "{kind:?}"
        );
    }
}

#[tokio::test]
async fn an_unversioned_procedure_and_repeated_calls_round_trip() {
    for kind in KINDS {
        let client = client_for(server(kind).await, kind);
        let plain = client::cratestack_schema::procedures::plain::Args {
            args: args("x").args,
        };
        for round in 0..3 {
            let reply = client.procedures().plain(&plain, &[]).await;
            assert_eq!(
                reply
                    .unwrap_or_else(|e| panic!("{kind:?} #{round}: {e}"))
                    .echo,
                "plain:x"
            );
        }
    }
}

#[tokio::test]
async fn an_idempotency_key_is_bound_and_accepted() {
    for kind in KINDS {
        let client = client_for(server(kind).await, kind);
        let reply = client
            .procedures()
            .ping(&args("keyed"), &[("Idempotency-Key", "k-1")])
            .await
            .unwrap_or_else(|error| panic!("{kind:?}: {error}"));
        assert!(reply.echo.starts_with("v2:keyed"), "{kind:?}");
    }
}
