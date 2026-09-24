//! The RFC 9728 Protected Resource Metadata routes.
//!
//! Served at both paths an MCP client tries, the path-suffixed one first
//! (`/.well-known/oauth-protected-resource/<resource path>`), then the root
//! one. The routes come from the resource identifier, not from where the
//! service is mounted, so a nested mount (`Router::nest("/api", ...)`) is
//! described correctly as long as this router is merged at the root. No
//! Origin or token check: the document is public by definition, and a
//! client reads it precisely because it has no token yet.

use axum::Router;
use axum::routing::get;
use http::HeaderValue;
use http::header::CONTENT_TYPE;

use super::resource::Resolved;

pub(crate) fn router(resource: &Resolved) -> Router {
    let mut router = Router::new();
    for path in &resource.well_known_paths {
        let document = resource.document.clone();
        router = router.route(
            path,
            get(move || {
                let document = document.clone();
                async move {
                    (
                        [(CONTENT_TYPE, HeaderValue::from_static("application/json"))],
                        document,
                    )
                }
            }),
        );
    }
    router
}
