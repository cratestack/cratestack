//! Real, compiling proof that the generated client this facade builds
//! actually works — not just that `cargo tree` shows the right shape. The
//! mock server below is a bare `tokio::net::TcpListener`, not `axum`: this
//! crate's whole point is proving `cratestack-axum` (and `axum` itself) is
//! absent from `cratestack-client`'s dependency graph, so its own test
//! suite doesn't reach for `axum` either, not even as a dev-dependency.

use client_only_verification::{build_client, schema};
use cratestack::CoolCodec;
use cratestack::client_rust::CborCodec;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[tokio::test]
async fn generated_client_lists_widgets_and_calls_a_procedure_over_real_http() {
    let (base_url, _server) = spawn_mock_server().await;
    let client = build_client(base_url);

    let widgets = client
        .widgets()
        .list(&[], &[])
        .await
        .expect("list should succeed");
    assert_eq!(widgets.len(), 1);
    assert_eq!(widgets[0].name, "Alpha");

    let reply = client
        .procedures()
        .ping(
            &schema::procedures::ping::Args {
                args: schema::PingArgs {
                    message: "hello".to_owned(),
                },
            },
            &[],
        )
        .await
        .expect("procedure call should succeed");
    assert_eq!(reply.echo, "hello");
}

async fn spawn_mock_server() -> (url::Url, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let addr = listener.local_addr().expect("listener should have addr");

    let handle = tokio::spawn(async move {
        loop {
            let Ok((socket, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let _ = serve_one(socket).await;
            });
        }
    });

    (
        url::Url::parse(&format!("http://{addr}")).expect("base url should parse"),
        handle,
    )
}

async fn serve_one(mut socket: tokio::net::TcpStream) -> std::io::Result<()> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos;
        }
        let n = socket.read(&mut chunk).await?;
        if n == 0 {
            break buf.len();
        }
        buf.extend_from_slice(&chunk[..n]);
    };

    let header_text = String::from_utf8_lossy(&buf[..header_end]);
    let request_line = header_text.split("\r\n").next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let path = parts.next().unwrap_or_default();

    let mut content_length = 0usize;
    for line in header_text.split("\r\n").skip(1) {
        if let Some((name, value)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse().unwrap_or(0);
            }
        }
    }
    let already_read = buf.len().saturating_sub(header_end + 4);
    let mut remaining = content_length.saturating_sub(already_read);
    while remaining > 0 {
        let n = socket.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        remaining = remaining.saturating_sub(n.min(remaining));
    }

    let body = match (method, path) {
        ("GET", "/widgets") => CborCodec
            .encode(&vec![schema::Widget {
                id: 1,
                name: "Alpha".to_owned(),
            }])
            .expect("widget list should encode"),
        ("POST", "/$procs/ping") => CborCodec
            .encode(&schema::PingReply {
                echo: "hello".to_owned(),
            })
            .expect("ping reply should encode"),
        _ => {
            let head = b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n";
            socket.write_all(head).await?;
            return socket.flush().await;
        }
    };

    let head = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: {}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        CborCodec::CONTENT_TYPE,
        body.len(),
    );
    socket.write_all(head.as_bytes()).await?;
    socket.write_all(&body).await?;
    socket.flush().await
}
