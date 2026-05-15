//! Server entry point — see `lib.rs` for the schema, procedures, and
//! `build_router` builder; see `tests/smoke.rs` for the wire-shape demos.

use rpc_procedures_example::build_router;
use std::net::SocketAddr;

#[tokio::main]
async fn main() {
    let app = build_router();
    let addr: SocketAddr = "127.0.0.1:3000".parse().expect("addr parses");
    println!("rpc-procedures-server listening on http://{addr}");
    println!("try: curl -X POST http://{addr}/rpc/procedure.greet \\");
    println!("       -H 'content-type: application/json' \\");
    println!("       -H 'accept: application/json' \\");
    println!("       -H 'x-auth-id: 1' \\");
    println!("       -d '{{\"args\":{{\"name\":\"world\"}}}}'");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("bind 127.0.0.1:3000");
    cratestack::axum::serve(listener, app)
        .await
        .expect("axum serve");
}
