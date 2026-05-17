//! Compile-time bundle of the Leptos UI's Trunk output.
//!
//! `build.rs` materializes the Trunk dist into `$OUT_DIR/ui-dist/` —
//! either from the `cratestack-studio-ui` sibling during local dev or
//! from the published `embedded-ui-dist.tar.gz`. `rust-embed` then
//! snapshots that directory at compile time. When no dist is
//! available the directory exists but is empty, `has_assets()`
//! returns false, and the server falls back to the placeholder page.
//!
//! Maintainer build prerequisites for refreshing the bundle:
//!
//! ```text
//! cargo install --locked trunk
//! rustup target add wasm32-unknown-unknown
//! just bundle-studio-ui
//! ```

use axum::Router;
use axum::body::Body;
use axum::http::{StatusCode, header};
use axum::response::Response;
use axum::routing::get;
use rust_embed::RustEmbed;

/// The Trunk `dist/` output, snapshotted at compile time.
#[derive(RustEmbed)]
#[folder = "$OUT_DIR/ui-dist"]
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
