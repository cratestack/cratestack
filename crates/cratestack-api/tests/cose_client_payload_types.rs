//! A generated client whose codec is JSON seals and opens JSON
//! (cratestack#1168), against the real generated server behind an envelope
//! layer that opted in, over REST and RPC alike (transport parity); and a
//! CBOR client is unchanged against the very same server. On a real listener.

mod cose_client_support;
#[path = "cose_client_support/rest_app.rs"]
mod rest_app;
#[path = "cose_client_support/rpc_app.rs"]
mod rpc_app;

use cose_client_support::{KINDS, alg_of};

#[tokio::test]
async fn a_json_client_round_trips_over_rest() {
    for kind in KINDS {
        let addr = rest_app::server_json(kind).await;
        let reply = rest_app::json_client_for(addr, kind)
            .procedures()
            .ping(&rest_app::args("hello"), &[])
            .await
            .unwrap_or_else(|error| panic!("{kind:?}: {error}"));
        assert_eq!(
            reply.echo,
            format!("v2:hello|signer=Some({})", alg_of(kind)),
            "{kind:?}"
        );
    }
}

#[tokio::test]
async fn a_json_client_round_trips_over_rpc() {
    for kind in KINDS {
        let addr = rpc_app::server_json(kind).await;
        let reply = rpc_app::json_client_for(addr, kind)
            .procedures()
            .ping(&rpc_app::args("hello"))
            .await
            .unwrap_or_else(|error| panic!("{kind:?}: {error}"));
        assert_eq!(
            reply.echo,
            format!("v2:hello|signer=Some({})", alg_of(kind)),
            "{kind:?}"
        );
    }
}

#[tokio::test]
async fn a_cbor_client_is_unchanged_against_the_same_servers() {
    for kind in KINDS {
        let rest = rest_app::client_for(rest_app::server_json(kind).await, kind);
        let reply = rest
            .procedures()
            .plain(
                &rest_app::client::cratestack_schema::procedures::plain::Args {
                    args: rest_app::args("x").args,
                },
                &[],
            )
            .await
            .unwrap_or_else(|error| panic!("rest {kind:?}: {error}"));
        assert_eq!(reply.echo, "plain:x");
        let rpc = rpc_app::client_for(rpc_app::server_json(kind).await, kind);
        let reply = rpc
            .procedures()
            .plain(
                &rpc_app::client::cratestack_schema::procedures::plain::Args {
                    args: rpc_app::args("y").args,
                },
            )
            .await
            .unwrap_or_else(|error| panic!("rpc {kind:?}: {error}"));
        assert_eq!(reply.echo, "plain:y");
    }
}

#[tokio::test]
async fn a_json_client_against_a_server_that_did_not_opt_in_is_refused_unsigned() {
    // The stock server allows CBOR alone inside the seal.
    let kind = KINDS[0];
    let client = rest_app::json_client_for(rest_app::server(kind).await, kind);
    let error = client
        .procedures()
        .ping(&rest_app::args("hello"), &[])
        .await
        .expect_err("415 before any key is looked up");
    assert!(
        matches!(
            error,
            cratestack_client_rust::ClientError::Envelope(
                cratestack_client_rust::EnvelopeError::Unsigned { status: 415 }
            )
        ),
        "{error:?}"
    );
}
