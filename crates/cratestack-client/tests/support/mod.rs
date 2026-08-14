//! Minimal single-request-at-a-time HTTP/1.1 mock server, built on a bare
//! `tokio::net::TcpListener` rather than `axum::serve` — this crate's whole
//! point is proving `cratestack-axum` (and `axum` itself) is absent from
//! its dependency graph, so its own test suite doesn't add `axum` back in
//! even as a dev-dependency. Good enough for the handful of canned
//! request/response round-trips this crate's integration tests need; not a
//! general-purpose HTTP server.
//!
//! Not auto-discovered as its own `cargo test` binary — Cargo's `tests/`
//! convention skips `tests/support/mod.rs` (the `mod.rs` filename), so this
//! is only reachable via `mod support;` from a real test file.

use std::collections::BTreeMap;

use cratestack_client_rust::CborCodec;
use cratestack_core::CratestackCodec;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

pub struct MockRequest {
    pub method: String,
    pub path: String,
    #[allow(dead_code)]
    pub headers: BTreeMap<String, String>,
    #[allow(dead_code)]
    pub body: Vec<u8>,
}

pub struct MockResponse {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
    /// Headers beyond `content-type` (e.g. `etag`) — issue #493's
    /// `get_with_response`/`update_with_response` round trip needs a
    /// mock server that can send these. Defaults to empty via
    /// `cbor_ok`/`not_found`, so existing call sites that build a
    /// `MockResponse` through those helpers are unaffected.
    pub extra_headers: Vec<(String, String)>,
}

/// `200 OK`, CBOR-encoded `value` — the shape every handler in this crate's
/// tests returns for a successful call.
#[allow(dead_code)]
pub fn cbor_ok<T: serde::Serialize>(value: &T) -> MockResponse {
    cbor_status(200, value)
}

/// CBOR-encoded `value` with an arbitrary status — cratestack#407's
/// `status_attribute_client_round_trip.rs` uses this to answer with a bare
/// `202` (no `200` anywhere in that exchange) and prove the generated
/// client's success-path decoding isn't hardcoded to exactly `200`.
#[allow(dead_code)]
pub fn cbor_status<T: serde::Serialize>(status: u16, value: &T) -> MockResponse {
    MockResponse {
        status,
        content_type: CborCodec::CONTENT_TYPE.to_owned(),
        body: CborCodec.encode(value).expect("value should encode"),
        extra_headers: Vec::new(),
    }
}

/// Same as [`cbor_ok`], plus caller-supplied extra headers (e.g. `etag`).
/// Only `generated_client_versioning.rs` uses this today — `#[allow(dead_code)]`
/// because `mod support;` is compiled fresh per test binary (Cargo's `tests/`
/// convention), so a helper unused by one binary still needs to compile
/// warning-free for the workspace's `-D warnings` gate.
#[allow(dead_code)]
pub fn cbor_ok_with_headers<T: serde::Serialize>(
    value: &T,
    extra_headers: Vec<(String, String)>,
) -> MockResponse {
    MockResponse {
        extra_headers,
        ..cbor_ok(value)
    }
}

#[allow(dead_code)]
pub fn not_found() -> MockResponse {
    MockResponse {
        status: 404,
        content_type: "text/plain".to_owned(),
        body: Vec::new(),
        extra_headers: Vec::new(),
    }
}

/// Spawns a background task accepting connections on an ephemeral
/// `127.0.0.1` port and answering every request with `handler(request)`.
/// The returned `JoinHandle` is a drop guard only (dropping it does not
/// abort the task, same as the `axum::serve` background-task pattern used
/// elsewhere in this repo's own test suites) — the task keeps running for
/// the rest of the test process.
pub async fn spawn_mock_server<F>(handler: F) -> (url::Url, tokio::task::JoinHandle<()>)
where
    F: Fn(MockRequest) -> MockResponse + Send + Sync + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let addr = listener.local_addr().expect("listener should have addr");
    let handler = std::sync::Arc::new(handler);

    let join = tokio::spawn(async move {
        loop {
            let Ok((socket, _)) = listener.accept().await else {
                break;
            };
            let handler = handler.clone();
            tokio::spawn(async move {
                let _ = serve_one(socket, &*handler).await;
            });
        }
    });

    let base_url = url::Url::parse(&format!("http://{addr}")).expect("base url should parse");
    (base_url, join)
}

async fn serve_one(
    mut socket: tokio::net::TcpStream,
    handler: &(dyn Fn(MockRequest) -> MockResponse + Send + Sync),
) -> std::io::Result<()> {
    let request = read_request(&mut socket).await?;
    let response = handler(request);
    write_response(&mut socket, &response).await
}

async fn read_request(socket: &mut tokio::net::TcpStream) -> std::io::Result<MockRequest> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        if let Some(pos) = find_header_terminator(&buf) {
            break pos;
        }
        let n = socket.read(&mut chunk).await?;
        if n == 0 {
            break buf.len();
        }
        buf.extend_from_slice(&chunk[..n]);
    };

    let header_bytes = &buf[..header_end];
    let header_text = String::from_utf8_lossy(header_bytes);
    let mut lines = header_text.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_owned();
    let path = parts.next().unwrap_or_default().to_owned();

    let mut headers = BTreeMap::new();
    let mut content_length = 0usize;
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim().to_owned();
            if name == "content-length" {
                content_length = value.parse().unwrap_or(0);
            }
            headers.insert(name, value);
        }
    }

    let terminator_len = 4; // "\r\n\r\n"
    let already_read_body = buf.len().saturating_sub(header_end + terminator_len);
    let mut body = buf[(header_end + terminator_len).min(buf.len())..].to_vec();
    let mut remaining = content_length.saturating_sub(already_read_body);
    while remaining > 0 {
        let n = socket.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        let take = n.min(remaining);
        body.extend_from_slice(&chunk[..take]);
        remaining -= take;
    }

    Ok(MockRequest {
        method,
        path,
        headers,
        body,
    })
}

fn find_header_terminator(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|window| window == b"\r\n\r\n")
}

async fn write_response(
    socket: &mut tokio::net::TcpStream,
    response: &MockResponse,
) -> std::io::Result<()> {
    let reason = match response.status {
        200 => "OK",
        202 => "Accepted",
        404 => "Not Found",
        412 => "Precondition Failed",
        500 => "Internal Server Error",
        _ => "Error",
    };
    let mut head = format!(
        "HTTP/1.1 {} {}\r\ncontent-type: {}\r\ncontent-length: {}\r\nconnection: close\r\n",
        response.status,
        reason,
        response.content_type,
        response.body.len(),
    );
    for (name, value) in &response.extra_headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    socket.write_all(head.as_bytes()).await?;
    socket.write_all(&response.body).await?;
    socket.flush().await
}
