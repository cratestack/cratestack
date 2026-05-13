//! Tauri 2 desktop shell for the `tauri-web` example.
//!
//! Two halves of the architecture:
//!
//! 1. **Webview side (sibling `tauri-web` wasm cdylib)** — hosts
//!    `cratestack-rusqlite` and exposes the local OPFS-backed model delegate
//!    to JavaScript. The webview's UI talks to the wasm module inside a
//!    Dedicated Worker (same shape as `embedded-browser-vite`).
//!
//! 2. **Native shell side (this crate)** — uses `include_client_schema!` to
//!    drive a typed HTTP client against a remote CrateStack service. The
//!    webview JS calls this via Tauri commands rather than firing `fetch`
//!    itself, so HTTPS clients, cert pinning, mutual TLS, secret storage,
//!    etc. live in trusted native code instead of leaking to the renderer.
//!
//! Splitting into `lib.rs` + a thin `main.rs` matches Tauri 2's convention
//! and makes mobile targets (iOS / Android Tauri) reuse the same `run()`
//! entry from a different binary host.

use cratestack::include_client_schema;
use cratestack_client_rust::{ClientConfig, CratestackClient};
use cratestack_codec_cbor::CborCodec;
use serde::Serialize;
use url::Url;

include_client_schema!("schema.cstack");

/// JSON-friendly view sent back to the webview over the Tauri IPC channel.
#[derive(Serialize)]
struct ArticleView {
    id: i64,
    title: String,
    published: bool,
}

impl From<cratestack_schema::Article> for ArticleView {
    fn from(value: cratestack_schema::Article) -> Self {
        Self {
            id: value.id,
            title: value.title,
            published: value.published,
        }
    }
}

/// Pings a remote CrateStack service and reads back the first page of
/// `Article` rows. Demonstrates the "trusted native HTTP client" pattern:
/// the webview never touches a remote endpoint directly — it always goes
/// through Tauri commands.
#[tauri::command]
async fn fetch_remote_articles(base_url: String) -> Result<Vec<ArticleView>, String> {
    let url = Url::parse(&base_url).map_err(|error| format!("bad url: {error}"))?;
    let runtime = CratestackClient::new(ClientConfig::new(url), CborCodec);
    let client = cratestack_schema::client::Client::new(runtime);
    let articles_client = client.articles();
    let rows = articles_client
        .list(&[("limit", "10")], &[])
        .await
        .map_err(|error| error.to_string())?;
    Ok(rows.into_iter().map(ArticleView::from).collect())
}

#[tauri::command]
fn surface_summary() -> serde_json::Value {
    serde_json::json!({
        "models": cratestack_schema::MODELS,
        "types": cratestack_schema::TYPES,
        "procedures": cratestack_schema::PROCEDURES,
    })
}

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            fetch_remote_articles,
            surface_summary
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
