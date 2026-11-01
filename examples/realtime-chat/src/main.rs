//! `realtime-chat-server`: the chat over HTTP (unary calls, SSE, WebSocket)
//! and WebTransport, against `DATABASE_URL`.
//!
//! WebTransport runs on a short-lived self-signed certificate made at
//! startup (`DevCertificate`, at most 14 days, which is what a browser's
//! `serverCertificateHashes` accepts). Its SHA-256 is printed, on one JSON
//! line with both addresses, for the page to pin. Production serves a real
//! certificate instead (design §6a.5).
//!
//! Environment: `DATABASE_URL` (required), `REALTIME_CHAT_HTTP` (default
//! `127.0.0.1:8080`), `REALTIME_CHAT_WT` (UDP, default `127.0.0.1:4433`),
//! `REALTIME_CHAT_STATIC` (a directory served at `/`, for a web client).

use std::error::Error;
use std::net::SocketAddr;

use cratestack::sqlx::postgres::PgPoolOptions;
use cratestack::webtransport::DevCertificate;

fn address(name: &str, default: &str) -> Result<SocketAddr, Box<dyn Error>> {
    Ok(std::env::var(name)
        .unwrap_or_else(|_| default.to_owned())
        .parse()?)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let url = std::env::var("DATABASE_URL").map_err(|_| "set DATABASE_URL")?;
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect(&url)
        .await?;
    cratestack::sqlx::query(realtime_chat_example::SCHEMA_SQL)
        .execute(&pool)
        .await?;
    let db = realtime_chat_example::Cratestack::builder(pool).build();

    let listener =
        tokio::net::TcpListener::bind(address("REALTIME_CHAT_HTTP", "127.0.0.1:8080")?).await?;
    let origin = format!("http://{}", listener.local_addr()?);
    let certificate = DevCertificate::generate(&["localhost", "127.0.0.1"])?;
    let webtransport = realtime_chat_example::webtransport(
        &db,
        address("REALTIME_CHAT_WT", "127.0.0.1:4433")?,
        certificate.certificate_chain,
        certificate.private_key,
        &origin,
    )?;
    let webtransport_url = format!(
        "https://127.0.0.1:{}/rpc/wt",
        webtransport.local_addr()?.port()
    );
    println!(
        "{}",
        serde_json::json!({
            "http": origin,
            "webtransport": webtransport_url,
            "sha256": certificate.sha256,
        })
    );
    tokio::spawn(webtransport.serve());

    let mut app = realtime_chat_example::router(db);
    if let Ok(dir) = std::env::var("REALTIME_CHAT_STATIC") {
        app = app.fallback_service(tower_http::services::ServeDir::new(dir));
    }
    cratestack::axum::serve(listener, app).await?;
    Ok(())
}
