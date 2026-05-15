//! Connects to a running `rpc-streaming-example` server and streams
//! the `procedure.ticks` cbor-seq response one item at a time.
//!
//! ### Run
//!
//! ```bash
//! # In one terminal:
//! cargo run -p rpc-streaming-example
//!
//! # In another:
//! REMOTE_URL=http://localhost:3001 cargo run -p rpc-streaming-client-rust-example
//! ```
//!
//! Without `REMOTE_URL` the binary prints what it would do and exits.

use std::sync::Arc;

use cratestack_client_rust::{ClientConfig, CratestackClient};
use cratestack_codec_cbor::CborCodec;
use rpc_streaming_client_rust_example::{
    StaticAuthId,
    cratestack_schema::{self, TickerArgs, procedures::ticks},
};
use url::Url;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let remote_url = match std::env::var("REMOTE_URL") {
        Ok(value) => value,
        Err(_) => {
            println!("REMOTE_URL not set.");
            println!("Start the server example with:");
            println!("    cargo run -p rpc-streaming-example");
            println!("Then re-run this binary with:");
            println!("    REMOTE_URL=http://localhost:3001 cargo run -p rpc-streaming-client-rust-example");
            return Ok(());
        }
    };
    let base_url = Url::parse(&remote_url)?;

    // Build the runtime with an authorizer that injects `x-auth-id: 1`
    // on every request — the server example authenticates positive
    // integers as caller-id. The authorizer flows through every
    // generated client method automatically.
    let runtime = CratestackClient::new(ClientConfig::new(base_url.clone()), CborCodec)
        .with_request_authorizer(Arc::new(StaticAuthId(1)));
    let client = cratestack_schema::client::Client::new(runtime);

    let args = ticks::Args {
        args: TickerArgs {
            start: 100,
            count: 10,
        },
    };

    println!(
        "Streaming `procedure.ticks` from {base_url} (start={}, count={}):",
        args.args.start, args.args.count,
    );
    println!();

    // The macro saw `procedure ticks(...): Tick[]` and emitted a
    // streaming method that returns `RpcStream<Tick>` — a bounded
    // `tokio::sync::mpsc::Receiver` that yields each cbor-seq frame as
    // it parses off the wire. No full-body buffering.
    let mut rx = client.procedures().ticks(&args).await?;

    let mut received = 0usize;
    while let Some(item) = rx.recv().await {
        match item {
            Ok(tick) => {
                println!("  index={:<4} value={}", tick.index, tick.value);
                received += 1;
            }
            Err(error) => {
                // Per-item errors (decode failure, mid-stream transport
                // error) are terminal. Non-2xx responses surface at
                // `client.procedures().ticks(...)` before the channel
                // ever opens.
                eprintln!("\nstream error after {received} items: {error}");
                return Err(error.into());
            }
        }
    }
    println!();
    println!("stream closed cleanly after {received} items");
    Ok(())
}
