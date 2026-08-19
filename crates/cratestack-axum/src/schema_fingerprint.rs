//! Drift-detection middleware for the `x-cratestack-schema-sha` header
//! (issue #178). Every generated client stamps its own `SCHEMA_SHA256`
//! constant (`SHA-256` of the `.cstack` source it was compiled against)
//! onto every request; this middleware compares that value against the
//! server's own constant and `tracing::warn!`s on a mismatch — nothing
//! more. It never rejects a request: a missing header (a client not yet
//! regenerated) is not itself a warning, and a present-but-different value
//! only ever produces a log line, never an error response. Applies to
//! every transport (`rest`/`rpc` alike), since nothing about schema drift
//! is transport-specific.
//!
//! Deliberately a plain [`axum::middleware::from_fn_with_state`] function,
//! not a hand-rolled `tower::Layer`/`Service` pair like
//! [`crate::idempotency::IdempotencyLayer`] or
//! [`crate::ratelimit::RateLimitLayer`] — those exist because they need
//! async state lookups (a store) and per-request state threading that
//! justifies the extra structure. This check is "compare a header to a
//! known string and maybe log," which `from_fn_with_state` covers in a
//! fraction of the code with no loss of correctness.

use axum::extract::{Request, State};
use axum::http::HeaderName;
use axum::middleware::Next;
use axum::response::Response;

/// `x-cratestack-schema-sha` — lowercase per HTTP/2 header-name convention;
/// `axum`/`http` normalize header name lookups case-insensitively either
/// way, but the constant form is what generated clients literally send.
pub const SCHEMA_SHA_HEADER: HeaderName = HeaderName::from_static("x-cratestack-schema-sha");

/// Wraps a router with the drift-detection check. `expected_sha` is the
/// server's own `SCHEMA_SHA256` constant (`'static`, baked in at macro-
/// expansion time — see `crates/cratestack-macros/src/include/server.rs`).
pub async fn warn_on_schema_mismatch(
    State(expected_sha): State<&'static str>,
    request: Request,
    next: Next,
) -> Response {
    if let Some(received) = request
        .headers()
        .get(&SCHEMA_SHA_HEADER)
        .and_then(|value| value.to_str().ok())
        && received != expected_sha
    {
        tracing::warn!(
            expected_schema_sha = expected_sha,
            received_schema_sha = received,
            "client and server schema SHA-256 differ — one side may be compiled against a \
             stale copy of the `.cstack` schema"
        );
    }
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use axum::Router;
    use axum::body::Body;
    use axum::http::{Request as HttpRequest, StatusCode};
    use axum::middleware::from_fn_with_state;
    use axum::routing::get;
    use tower::ServiceExt;

    use super::{SCHEMA_SHA_HEADER, warn_on_schema_mismatch};

    fn app() -> Router {
        Router::new()
            .route("/", get(|| async { "ok" }))
            .layer(from_fn_with_state("expected-sha", warn_on_schema_mismatch))
    }

    #[tokio::test]
    async fn matching_header_passes_through_with_200() {
        let response = app()
            .oneshot(
                HttpRequest::builder()
                    .uri("/")
                    .header(SCHEMA_SHA_HEADER, "expected-sha")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn mismatched_header_still_passes_through_with_200() {
        // The whole point: a mismatch warns, it never rejects.
        let response = app()
            .oneshot(
                HttpRequest::builder()
                    .uri("/")
                    .header(SCHEMA_SHA_HEADER, "different-sha")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn missing_header_passes_through_with_200() {
        let response = app()
            .oneshot(HttpRequest::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
}
