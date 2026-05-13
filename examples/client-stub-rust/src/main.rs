//! Standalone HTTP-client binary built on `cratestack-client-rust` via
//! `include_client_schema!`.
//!
//! Shape: this is a Rust service that **does not** own the database. It
//! consumes another CrateStack service's `.cstack` schema as a *contract*,
//! generates a typed Rust client from it, and uses that client to talk to
//! the remote service over CBOR.
//!
//! No sqlx. No axum. No procedures. No FromRow impls. Smallest possible
//! dependency surface for a Rust HTTP consumer.
//!
//! ### Run
//!
//! ```bash
//! # Point at any CrateStack service exposing the same schema:
//! REMOTE_URL=http://localhost:3000 cargo run -p client-stub-rust-example
//! ```
//!
//! Without `REMOTE_URL`, the example prints the generated typed surface
//! (model fields, procedure names) and exits — useful for verifying
//! compilation and previewing the contract.

use cratestack::include_client_schema;
use cratestack_client_rust::{ClientConfig, CratestackClient};
use cratestack_codec_cbor::CborCodec;
use url::Url;

include_client_schema!("schema.cstack");

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let remote_url = match std::env::var("REMOTE_URL") {
        Ok(value) => value,
        Err(_) => {
            print_surface();
            return Ok(());
        }
    };
    let base_url = Url::parse(&remote_url)?;

    let runtime = CratestackClient::new(ClientConfig::new(base_url), CborCodec);
    let client = cratestack_schema::client::Client::new(runtime);

    // Typed list call. The macro emits one method per generated route;
    // `.list(query_params, headers)` is the canonical model-list shape.
    let posts = client.posts().list(&[("limit", "10")], &[]).await?;
    println!("fetched {} posts", posts.len());
    for post in posts.iter().take(3) {
        println!(
            "  #{:<4} {:<40} (published={})",
            post.id, post.title, post.published
        );
    }
    Ok(())
}

fn print_surface() {
    println!("REMOTE_URL not set. Generated typed surface:");
    println!("  models    = {:?}", cratestack_schema::MODELS);
    println!("  types     = {:?}", cratestack_schema::TYPES);
    println!("  procedures = {:?}", cratestack_schema::PROCEDURES);
    println!();
    println!("Set REMOTE_URL=http://… to call a live CrateStack service.");
}
