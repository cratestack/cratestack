//! Serves the signed, compact-capable RPC service on 127.0.0.1:3000
//! (`PORT` overrides). Prints one line per call: what crossed the wire.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use signed_compact_server::{DEMO_SECRET, Options, app};

#[tokio::main]
async fn main() {
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|port| port.parse().ok())
        .unwrap_or(3000);
    let secret = std::env::var("SIGNED_COMPACT_SECRET")
        .map(String::into_bytes)
        .unwrap_or_else(|_| DEMO_SECRET.to_vec());
    let calls = AtomicUsize::new(0);
    let options = Options {
        compact: true,
        observer: Some(Arc::new(move |wire| {
            let call = calls.fetch_add(1, Ordering::Relaxed) + 1;
            println!(
                "call {call}: request payload type = {}, response payload type = {}",
                shown(&wire.request),
                shown(&wire.response)
            );
        })),
    };
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr).await.expect("bind");
    println!("signed-compact-server listening on http://{addr} (COSE Mac0, audience \"payments\")");
    cratestack::axum::serve(listener, app(&secret, options))
        .await
        .expect("serve");
}

fn shown(payload_type: &str) -> &str {
    if payload_type.is_empty() {
        "(none: plain CBOR, named keys)"
    } else {
        payload_type
    }
}
