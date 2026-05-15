//! Server entry — see `lib.rs` for the handlers, and `tests/smoke.rs`
//! for the batch wire-shape demos.

use rpc_batch_example::build_router;
use std::net::SocketAddr;

#[tokio::main]
async fn main() {
    let app = build_router();
    let addr: SocketAddr = "127.0.0.1:3002".parse().expect("addr parses");
    println!("rpc-batch-server listening on http://{addr}");
    println!();
    println!("# A batch of three ops in one POST:");
    println!("cat <<'EOF' | curl -X POST http://{addr}/rpc/batch \\", );
    println!("  -H 'content-type: application/json' \\");
    println!("  -H 'accept: application/json' \\");
    println!("  -H 'x-auth-id: 1' \\");
    println!("  --data-binary @-");
    println!("[");
    println!("  {{ \"id\": 1, \"op\": \"procedure.add\",      \"input\": {{\"args\": {{\"a\": 2, \"b\": 3}} }} }},");
    println!("  {{ \"id\": 2, \"op\": \"procedure.divide\",   \"input\": {{\"args\": {{\"numerator\": 10, \"denominator\": 0}} }} }},");
    println!("  {{ \"id\": 3, \"op\": \"procedure.multiply\", \"input\": {{\"args\": {{\"a\": 4, \"b\": 5}} }} }}");
    println!("]");
    println!("EOF");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("bind 127.0.0.1:3002");
    cratestack::axum::serve(listener, app)
        .await
        .expect("axum serve");
}
