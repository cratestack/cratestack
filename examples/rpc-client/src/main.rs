//! Connects to a running `rpc-procedures-example` server and drives both
//! RPC client surfaces end-to-end: a single unary call followed by a
//! batched round-trip over `POST /rpc/batch`.
//!
//! ### Run
//!
//! ```bash
//! # In one terminal:
//! cargo run -p rpc-procedures-example
//!
//! # In another:
//! REMOTE_URL=http://localhost:3000 cargo run -p rpc-client-example
//! ```
//!
//! Without `REMOTE_URL` the binary prints what it would do and exits.

use std::sync::Arc;

use cratestack_client_rust::{ClientConfig, CratestackClient};
use cratestack_codec_cbor::CborCodec;
use rpc_client_example::{
    StaticAuthId,
    cratestack_schema::{self, CounterDelta, GreetArgs, procedures},
};
use url::Url;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let remote_url = match std::env::var("REMOTE_URL") {
        Ok(value) => value,
        Err(_) => {
            println!("REMOTE_URL not set.");
            println!("Start the server example with:");
            println!("    cargo run -p rpc-procedures-example");
            println!("And re-run this binary with:");
            println!("    REMOTE_URL=http://localhost:3000 cargo run -p rpc-client-example");
            return Ok(());
        }
    };
    let base_url = Url::parse(&remote_url)?;

    // Build the runtime with an authorizer that injects `x-auth-id: 1` on
    // every request — the server example authenticates positive integers
    // as caller-id. The authorizer flows through every generated client
    // method automatically.
    let runtime = CratestackClient::new(ClientConfig::new(base_url.clone()), CborCodec)
        .with_request_authorizer(Arc::new(StaticAuthId(1)));
    let client = cratestack_schema::client::Client::new(runtime);

    // ------------------------------------------------------------------
    // 1) UNARY call — `.await` on a BatchableCall fires one request.
    // ------------------------------------------------------------------
    let greet_args = procedures::greet::Args {
        args: GreetArgs {
            name: "world".to_owned(),
        },
    };
    println!("Unary call: procedure.greet(\"world\")");
    let reply = client.procedures().greet(&greet_args).await?;
    println!("  -> {reply:?}");
    println!();

    // ------------------------------------------------------------------
    // 2) BATCH call — prepare two calls, queue them into one builder, and
    //    collect both results from a single `/rpc/batch` round-trip.
    // ------------------------------------------------------------------
    println!("Batch call: two increments over one /rpc/batch round-trip");

    let mut batch = client.batch();

    // A BatchableCall can either be `.await`ed (unary, above) OR `.queue`d
    // into a BatchBuilder. Queuing returns a BatchHandle you use later to
    // pluck that call's result out of the shared response.
    let inc_5 = client.procedures().increment(&procedures::increment::Args {
        args: CounterDelta { by: 5 },
    });
    let inc_3 = client.procedures().increment(&procedures::increment::Args {
        args: CounterDelta { by: 3 },
    });

    let handle_5 = inc_5.queue(&mut batch);
    let handle_3 = inc_3.queue(&mut batch);

    // One POST /rpc/batch carries both ops; results arrive per-frame.
    let mut results = batch.send().await?;

    let total_5 = results.take(handle_5)?;
    let total_3 = results.take(handle_3)?;
    println!("  increment(5) -> {:?}", total_5);
    println!("  increment(3) -> {:?}", total_3);

    Ok(())
}
