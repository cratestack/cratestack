//! The generated RPC client seals and opens (cratestack#1007): unary calls
//! and `/rpc/batch`, against the real generated `rpc_router` with the
//! envelope layer in `Required` mode, on a real listener. The REST twin is
//! `cose_client_rest.rs`, and the two must stay in step (transport parity).

mod cose_client_support;
#[path = "cose_client_support/rpc_app.rs"]
mod rpc_app;

use cose_client_support::{KINDS, alg_of};
use rpc_app::{args, client, client_for, server};

#[tokio::test]
async fn a_signed_unary_call_round_trips_in_every_mode() {
    for kind in KINDS {
        let client = client_for(server(kind).await, kind);
        let reply = client
            .procedures()
            .ping(&args("hello"))
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
async fn the_batch_route_is_one_signed_round_trip() {
    for kind in KINDS {
        let client = client_for(server(kind).await, kind);
        let mut batch = client.batch();
        let first = client.procedures().ping(&args("a")).queue(&mut batch);
        let second = client
            .procedures()
            .plain(&client::cratestack_schema::procedures::plain::Args {
                args: args("b").args,
            })
            .queue(&mut batch);
        let mut results = batch
            .send()
            .await
            .unwrap_or_else(|error| panic!("{kind:?}: {error}"));
        assert_eq!(
            results.take(first).expect("first").echo,
            format!("v2:a|signer=Some({})", alg_of(kind)),
            "{kind:?}"
        );
        assert_eq!(results.take(second).expect("second").echo, "plain:b");
    }
}
