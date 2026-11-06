//! Calls `signed-compact-server` twice with a sealed, compact-capable client.
//!
//! ```bash
//! cargo run -p signed-compact-server        # terminal 1
//! cargo run -p signed-compact-client        # terminal 2
//! ```
//!
//! `REMOTE_URL` (default http://127.0.0.1:3000) and `SIGNED_COMPACT_SECRET`
//! override the target and the demo secret.

use signed_compact_client::{DEMO_SECRET, client, transfer};
use url::Url;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let base = std::env::var("REMOTE_URL").unwrap_or_else(|_| "http://127.0.0.1:3000".to_owned());
    let secret = std::env::var("SIGNED_COMPACT_SECRET")
        .map(String::into_bytes)
        .unwrap_or_else(|_| DEMO_SECRET.to_vec());
    let client = client(Url::parse(&base)?, &secret, true);
    let args = transfer();

    // Both calls are COSE-sealed: the outer Content-Type is application/cose
    // either way. The compact opt-in is adaptive and shows in the inner
    // payload type, which the client cannot observe itself: the server's
    // own log (and tests/sealed_compact.rs) prove what follows.
    let first = client.procedures().transfer_funds(&args).await?;
    println!("call 1 -> {first:?}");
    println!("  sent sealed with NAMED keys (plain CBOR); the server answered COMPACT.");

    let second = client.procedures().transfer_funds(&args).await?;
    println!("call 2 -> {second:?}");
    println!("  sent sealed with COMPACT keys (the client learned from call 1's answer).");
    Ok(())
}
