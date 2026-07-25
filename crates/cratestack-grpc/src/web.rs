//! gRPC-Web + CORS wiring — `docs/design/protobuf.md` §7.4. `tonic-web`'s
//! [`GrpcWebLayer`] translates the browser-facing gRPC-Web wire protocol
//! into what the inner tonic service already speaks (real gRPC framing);
//! it does **not** add any CORS headers itself (confirmed against
//! `tonic-web-0.13.1`'s own source — `GrpcWebService` never touches
//! `Access-Control-*`, and its own test suite composes a `CorsLayer`
//! explicitly), so the CORS layer here is what actually makes the
//! translated response usable from a browser.
//!
//! Ticket #171 already proved `tonic::service::Routes::into_axum_router()`
//! merges into a plain `axum::Router`; this module proves the same for
//! layering `GrpcWebLayer` + CORS on top (`docs/design/protobuf.md` §7.4's
//! "in-process `tonic-web`, not an external proxy" call), independent of
//! any macro-generated service — see [`apply_grpc_web`]'s tests.

use axum::Router;
use http::HeaderName;
use tonic_web::GrpcWebLayer;
use tower_http::cors::{Any, CorsLayer};

/// Response headers a gRPC-Web browser client must be able to *read* to
/// see a call's real outcome. gRPC signals completion via
/// `grpc-status`/`grpc-message` — folded into the gRPC-Web trailer frame
/// by `tonic-web`, §7.4 point 1 — and structured error detail rides
/// `grpc-status-details-bin` ([`crate::status_details`]). None of the
/// three reach a browser's `fetch` `Response.headers` unless they're
/// listed in `Access-Control-Expose-Headers`: miss this and every
/// browser call "succeeds" from the client's point of view while
/// silently discarding the actual gRPC outcome — §7.4 point 2 names this
/// the single highest-severity failure mode this crate can ship, because
/// it fails opaquely (200-shaped response, no server-side signal) rather
/// than loudly.
pub const GRPC_WEB_EXPOSED_HEADERS: [&str; 3] =
    ["grpc-status", "grpc-message", "grpc-status-details-bin"];

/// Builds the CORS layer every gRPC-Web-mounted router needs. Origin,
/// method, and header allow-policy are permissive (`Any`) deliberately —
/// restricting the origin set is a deployment-time operational concern
/// the generated code has no basis to guess at (a schema doesn't know
/// which origins its consumers will run on). The exposed-headers set is
/// the opposite: it is structurally required by the gRPC-Web binding
/// itself (see [`GRPC_WEB_EXPOSED_HEADERS`]) and must not be
/// configurable away, so it is not a parameter here.
pub fn grpc_web_cors_layer() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any)
        .expose_headers(GRPC_WEB_EXPOSED_HEADERS.map(HeaderName::from_static))
}

/// Wraps an already-mounted gRPC `axum::Router` (`super::service::
/// into_router` in macro-generated code) with the in-process gRPC-Web
/// translation layer and the CORS layer browsers need to read the
/// translated response.
///
/// Layer order matters: `GrpcWebLayer` sits closest to the tonic service
/// (it translates gRPC-Web bytes into real gRPC framing on the way in and
/// back on the way out), CORS sits outermost so it sees the raw incoming
/// request's `Origin` header and stamps the final outgoing response —
/// `axum::Router::layer` makes the last call in the chain the outermost
/// layer, so `.layer(GrpcWebLayer::new()).layer(cors)` is
/// request -> CORS -> GrpcWeb -> tonic service -> GrpcWeb -> CORS -> response.
pub fn apply_grpc_web(router: Router) -> Router {
    router
        .layer(GrpcWebLayer::new())
        .layer(grpc_web_cors_layer())
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::response::IntoResponse;
    use axum::routing::{Router, post};
    use http::{Request, StatusCode, header};
    use tower::ServiceExt;

    use super::apply_grpc_web;

    /// A trivial inner service standing in for a macro-generated
    /// `ApiServer` — this test's job is proving the *layering*, not any
    /// particular gRPC method body.
    fn test_router() -> Router {
        apply_grpc_web(Router::new().route(
            "/pkg.Api/Method",
            post(|| async { StatusCode::OK.into_response() }),
        ))
    }

    /// The load-bearing assertion for Part A: a real (non-preflight)
    /// response from a gRPC-Web-mounted router exposes exactly the three
    /// headers a browser needs to read the call's outcome. Without this,
    /// `docs/design/protobuf.md` §7.4 point 2's failure mode is silent —
    /// this test would not catch it by "does it compile," only by
    /// actually asserting the header value.
    #[tokio::test]
    async fn exposes_grpc_status_headers_on_actual_response() {
        let router = test_router();
        let request = Request::builder()
            .method("POST")
            .uri("/pkg.Api/Method")
            .header(header::ORIGIN, "http://example.com")
            .body(Body::empty())
            .unwrap();

        let response = router.oneshot(request).await.unwrap();

        let exposed = response
            .headers()
            .get(header::ACCESS_CONTROL_EXPOSE_HEADERS)
            .expect("Access-Control-Expose-Headers must be present on a CORS response")
            .to_str()
            .unwrap();

        for header_name in super::GRPC_WEB_EXPOSED_HEADERS {
            assert!(
                exposed.contains(header_name),
                "expected '{header_name}' in Access-Control-Expose-Headers, got '{exposed}'"
            );
        }
    }

    /// A preflight `OPTIONS` request must not carry
    /// `Access-Control-Expose-Headers` — that header only describes the
    /// *actual* response (`tower-http`'s own implementation restricts it
    /// to non-preflight calls). Asserting the negative here pins the
    /// distinction so a future refactor doesn't "fix" preflight by
    /// copying the actual-response header set onto it.
    #[tokio::test]
    async fn preflight_response_has_no_expose_headers() {
        let router = test_router();
        let request = Request::builder()
            .method("OPTIONS")
            .uri("/pkg.Api/Method")
            .header(header::ORIGIN, "http://example.com")
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
            .body(Body::empty())
            .unwrap();

        let response = router.oneshot(request).await.unwrap();

        assert!(
            response
                .headers()
                .get(header::ACCESS_CONTROL_EXPOSE_HEADERS)
                .is_none()
        );
    }

    /// `allow_origin(Any)` echoes back as a literal `*` — confirms the
    /// permissive origin policy documented on [`super::grpc_web_cors_layer`]
    /// is actually wired up, not just the exposed-headers set. Wildcard
    /// `Access-Control-Allow-Origin` is only legal without
    /// `Access-Control-Allow-Credentials: true`, which this layer never
    /// sets — consistent with the CORS crate's own panic-on-misuse guard
    /// (`ensure_usable_cors_rules`) not firing here.
    #[tokio::test]
    async fn allow_origin_is_wildcard() {
        let router = test_router();
        let request = Request::builder()
            .method("POST")
            .uri("/pkg.Api/Method")
            .header(header::ORIGIN, "http://example.com")
            .body(Body::empty())
            .unwrap();

        let response = router.oneshot(request).await.unwrap();

        assert_eq!(
            response
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .expect("Access-Control-Allow-Origin must be present"),
            "*"
        );
    }
}
