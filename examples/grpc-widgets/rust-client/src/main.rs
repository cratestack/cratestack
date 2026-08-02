//! Native `tonic`-based Rust gRPC client for the `grpc-widgets` example
//! schema, generated via `include_client_schema!` (ticket #209) — the
//! Rust sibling of `ts-client/` (gRPC-Web, ticket #172) and `dart-client/`
//! (native `package:grpc`, ticket #210) in the parent example directory.
//!
//! Shape: this is a Rust *consumer*, not the server — it does not own the
//! database, treats `../schemas/widgets.cstack` purely as a contract (the
//! same `.cstack`-as-contract model `client-stub-rust-example` uses for
//! REST/RPC), and speaks real binary gRPC over HTTP/2 via `tonic`.
//!
//! ### Run
//!
//! ```bash
//! # In one shell:
//! export DATABASE_URL=postgres://cratestack:cratestack@localhost:55432/cratestack_test
//! cargo run -p grpc-widgets-example
//!
//! # In another:
//! cargo run -p grpc-widgets-rust-client-example
//! ```
//!
//! Without a reachable server at `http://127.0.0.1:50061`, `connect()`
//! fails fast and this prints the generated typed surface instead —
//! mirroring `client-stub-rust-example`'s `REMOTE_URL`-gated shape, useful
//! for verifying compilation and previewing the contract without a live
//! server.

use std::sync::Arc;

use cratestack::client_rust::{AuthorizationRequest, ClientError, RequestAuthorizer};
use cratestack::include_client_schema;

include_client_schema!("../schemas/widgets.cstack");

/// The example schema gates every CRUD verb on `auth() != null`
/// (`schemas/widgets.cstack`'s `@@allow` rules), and the server's
/// `HeaderAuthProvider` (`src/main.rs`) reads a plain `x-auth-id` header
/// out of gRPC metadata — the same header-driven auth every other
/// `grpc-widgets` client (`ts-client`, `dart-client`) sends. This
/// authorizer is deliberately the simplest possible `RequestAuthorizer`
/// impl: a static header, ignoring the canonical-request-string entirely.
/// A real deployment would sign `request.canonical_request` (the same
/// value `cratestack-client-rust::grpc::canonical` computes from this
/// call's unframed prost-encoded bytes) instead of trusting a bare header.
struct StaticAuthIdHeader;

impl RequestAuthorizer for StaticAuthIdHeader {
    fn authorize(
        &self,
        _request: &AuthorizationRequest,
    ) -> Result<Vec<(String, String)>, ClientError> {
        Ok(vec![("x-auth-id".to_owned(), "1".to_owned())])
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let endpoint =
        std::env::var("GRPC_ENDPOINT").unwrap_or_else(|_| "http://127.0.0.1:50061".to_owned());

    let client = match cratestack_schema::grpc::Client::connect(endpoint.clone()).await {
        Ok(client) => client.with_request_authorizer(Arc::new(StaticAuthIdHeader)),
        Err(error) => {
            println!("Could not connect to {endpoint} ({error}).");
            print_surface();
            return Ok(());
        }
    };

    let created = client
        .widgets()
        .create(&cratestack_schema::inputs::CreateWidgetInput {
            id: 9001,
            name: "gizmo".to_owned(),
        })
        .await?;
    println!("created  #{:<4} {}", created.id, created.name);

    let fetched = client.widgets().get(&created.id).await?;
    println!("fetched  #{:<4} {}", fetched.id, fetched.name);

    let updated = client
        .widgets()
        .update(
            &created.id,
            &cratestack_schema::inputs::UpdateWidgetInput {
                name: Some("gizmo-v2".to_owned()),
            },
        )
        .await?;
    println!("updated  #{:<4} {}", updated.id, updated.name);

    let page = client
        .widgets()
        .list(&cratestack::rpc::RpcListInput {
            limit: Some(10),
            ..Default::default()
        })
        .await?;
    println!("listed   {} widget(s)", page.items.len());
    for widget in page.items.iter().take(3) {
        println!("  #{:<4} {}", widget.id, widget.name);
    }

    let deleted = client.widgets().delete(&created.id).await?;
    println!("deleted  #{:<4} {}", deleted.id, deleted.name);

    match client.widgets().get(&created.id).await {
        Ok(widget) => println!("unexpected: get-after-delete succeeded: {widget:?}"),
        Err(error) => println!("get-after-delete correctly failed: {error}"),
    }

    Ok(())
}

fn print_surface() {
    println!("Generated typed surface:");
    println!("  models = {:?}", cratestack_schema::MODELS);
    println!();
    println!("Set GRPC_ENDPOINT=http://… (default http://127.0.0.1:50061) to call a live server.");
}
