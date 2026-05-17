//! Compile-time bundle of the Leptos UI's Trunk output.
//!
//! Gated behind the `embed-ui` cargo feature. Build prerequisites:
//!
//! ```text
//! cargo install trunk
//! rustup target add wasm32-unknown-unknown
//! (cd crates/cratestack-studio-ui && trunk build --release)
//! cargo build -p cratestack-cli --features cratestack-studio/embed-ui
//! ```
//!
//! `rust-embed`'s proc-macro reads the directory at compile time, so
//! `trunk build` must run before the studio binary is compiled. Without
//! the `embed-ui` feature, the binary serves the Phase 1b stub `/`
//! page instead.

use axum::Router;
use axum::body::Body;
use axum::http::{StatusCode, header};
use axum::response::Response;
use axum::routing::get;
use rust_embed::RustEmbed;

/// The Trunk `dist/` output, snapshotted at compile time.
#[derive(RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/../cratestack-studio-ui/dist"]
struct UiAssets;

/// Mount the bundled UI on the given router under `/`. Falls back to
/// `index.html` for any unknown path so the SPA can handle in-page
/// routing.
pub fn mount<S>(router: Router<S>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router
        .route("/", get(serve_index))
        .route("/{*path}", get(serve_path))
}

async fn serve_index() -> Response {
    serve("index.html").await
}

async fn serve_path(axum::extract::Path(path): axum::extract::Path<String>) -> Response {
    if path.starts_with("api/") {
        // Defense-in-depth — API routes are mounted before us, but if
        // somebody reorders the router we shouldn't shadow the JSON
        // surface with a 200 OK for index.html.
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::empty())
            .expect("response builds");
    }
    serve(&path).await
}

async fn serve(path: &str) -> Response {
    match UiAssets::get(path) {
        Some(file) => {
            let mime = file.metadata.mimetype();
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, mime)
                .body(Body::from(file.data.into_owned()))
                .expect("response builds")
        }
        None => {
            // SPA-style fallback: anything we don't recognize gets the
            // root document. The browser's client-side routing takes
            // it from there.
            if let Some(index) = UiAssets::get("index.html") {
                return Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
                    .body(Body::from(index.data.into_owned()))
                    .expect("response builds");
            }
            Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::empty())
                .expect("response builds")
        }
    }
}

/// Whether the bundle is non-empty. Used by the server smoke check.
pub fn has_assets() -> bool {
    UiAssets::iter().next().is_some()
}
