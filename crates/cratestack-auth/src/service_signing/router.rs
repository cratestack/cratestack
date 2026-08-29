//! Mountable axum router serving a service's own JWKS document.
//!
//! Gated behind this crate's `axum` Cargo feature (default-on) — see the
//! crate root doc comment.

use std::sync::Arc;

use axum::{Router, http::header, response::IntoResponse, routing::get};

use crate::JwksDocument;

/// Mountable axum router that serves the given JWKS document at
/// `/jwks.json` and `/.well-known/jwks.json`.
///
/// Service binaries typically:
///
/// ```text
/// let signing_key = ServiceSigningKey::from_env(
///     "vendor-service",
///     "vendor-service-v1",
///     "MY_SERVICE_SIGNING_KEY",
/// )?;
/// let app = Router::new()
///     .merge(jwks_router(signing_key.jwks_document()))
///     .route("/healthz", get(healthz));
/// ```
///
/// The `JwksDocument` is captured by value at mount time, so on
/// rotation you'd either rebuild the router or merge a router that
/// reads JWKS from a shared `Arc<RwLock<JwksDocument>>` — the
/// helper covers the static-document case, which is what every
/// service needs in steady state.
pub fn jwks_router(document: JwksDocument) -> Router {
    let document = Arc::new(document);
    let document_alt = document.clone();
    Router::new()
        .route(
            "/jwks.json",
            get({
                let document = document.clone();
                move || serve_jwks(document.clone())
            }),
        )
        .route(
            "/.well-known/jwks.json",
            get({
                let document = document_alt.clone();
                move || serve_jwks(document.clone())
            }),
        )
}

async fn serve_jwks(document: Arc<JwksDocument>) -> impl IntoResponse {
    let body = serde_json::to_vec(&*document).unwrap_or_else(|_| b"{\"keys\":[]}".to_vec());
    (
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::CACHE_CONTROL, "public, max-age=300"),
        ],
        body,
    )
}
