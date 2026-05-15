//! Server entry — see `lib.rs` for the schema, handler, and `build_router`;
//! see `tests/smoke.rs` for the wire-shape demos covering both unary
//! and streamed responses on the same op.

use rpc_streaming_example::build_router;
use std::net::SocketAddr;

#[tokio::main]
async fn main() {
    let app = build_router();
    let addr: SocketAddr = "127.0.0.1:3001".parse().expect("addr parses");
    println!("rpc-streaming-server listening on http://{addr}");
    println!();
    println!("# single CBOR Vec<Tick> (default Accept):");
    println!("curl -X POST http://{addr}/rpc/procedure.ticks \\");
    println!("  -H 'content-type: application/cbor' \\");
    println!("  -H 'x-auth-id: 1' \\");
    println!("  --data-binary @<(cbor-encode '{{\"args\":{{\"start\":10,\"count\":5}}}}') \\");
    println!("  -o /tmp/ticks.cbor");
    println!();
    println!("# streamed cbor-seq chunks:");
    println!("curl -X POST http://{addr}/rpc/procedure.ticks \\");
    println!("  -H 'content-type: application/cbor' \\");
    println!("  -H 'accept: application/cbor-seq' \\");
    println!("  -H 'x-auth-id: 1' \\");
    println!("  --data-binary @<(cbor-encode '{{\"args\":{{\"start\":10,\"count\":5}}}}') \\");
    println!("  --no-buffer");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("bind 127.0.0.1:3001");
    cratestack::axum::serve(listener, app)
        .await
        .expect("axum serve");
}
