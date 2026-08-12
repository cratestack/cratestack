//! Rate-limit filter for REST transport: check if a route is exempt
//! from rate limiting based on its `RouteTransportDescriptor.rate_limited_by_default`.
//!
//! REST resolves op identity very differently from RPC (`rpc_ops_filter`):
//! there is no single `/rpc/{op_id}` path segment to read, so this filter
//! keys off [`axum::extract::MatchedPath`] — the route *pattern* the
//! request matched (e.g. `/widgets/{id}`), not the concrete request path
//! (e.g. `/widgets/42`). `RouteTransportDescriptor::path` is emitted in
//! that same `{param}` shape (see `cratestack-macros/src/transport/rest.rs`),
//! so the two compare directly with no path-param parsing needed.
//!
//! # `Router::layer` and `Router::route_layer` both work
//!
//! Unlike some middleware, `MatchedPath` is populated for either mount
//! method — axum's `Router::layer` applies the middleware to each
//! endpoint's `Route` individually (same as `route_layer`), it just
//! *also* wraps the router's fallback (404) service, which `route_layer`
//! does not. Practically: with `route_layer`, a request that matches no
//! route skips this filter (and the rate limiter) entirely; with `layer`,
//! it still runs the filter, finds no match, and fails closed (rate-limits
//! the 404). Either is safe — pick based on whether 404s should count
//! against the budget.
use axum::extract::{MatchedPath, Request};
use cratestack_core::RouteTransportDescriptor;

/// Build a rate-limit filter function for REST schemas.
///
/// Returns a function that:
/// - Reads the matched route pattern via [`MatchedPath`] (populated by
///   axum under both `Router::layer` and `Router::route_layer` — see
///   module docs).
/// - Looks up the route (matched pattern + HTTP method) in the provided
///   descriptors.
/// - Returns `false` (exempt) if `rate_limited_by_default` is false.
/// - Returns `true` (apply rate limit) if the route participates, or if
///   lookup fails for any reason.
///
/// Fails closed: if `MatchedPath` is absent (the request matched no
/// route — a 404) or the route isn't found in `routes` (a schema/router
/// mismatch), the request is rate-limited. This prevents accidental
/// exemptions from missing data or misconfiguration.
pub fn build_rest_ops_filter(
    routes: &'static [RouteTransportDescriptor],
) -> impl Fn(&Request) -> bool + Send + Sync {
    move |req: &Request| {
        let Some(matched) = req.extensions().get::<MatchedPath>() else {
            // No matched path: the request hit no route (a 404). Fail closed.
            return true;
        };
        let path = matched.as_str();
        let method = req.method().as_str();

        match routes
            .iter()
            .find(|route| route.method == method && route.path == path)
        {
            Some(route) => route.rate_limited_by_default,
            None => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use axum::Router;
    use axum::body::Body;
    use axum::extract::ConnectInfo;
    use axum::http::{Request as HttpRequest, StatusCode};
    use axum::routing::{get, post};
    use cratestack_core::{RouteTransportCapabilities, RouteTransportDescriptor};
    use tower::ServiceExt;

    use super::build_rest_ops_filter;
    use crate::ratelimit::{InMemoryRateLimitStore, RateLimitConfig, RateLimitLayer};

    /// Every request in this module that actually reaches the rate-limit
    /// store (i.e. isn't exempted by the filter under test) needs a
    /// verifiable caller identity — cratestack#416 made the default key fn
    /// refuse requests with neither an `Authorization` header nor a
    /// `ConnectInfo<SocketAddr>` peer, and `oneshot` never populates
    /// `ConnectInfo` on its own.
    fn with_peer(mut req: HttpRequest<Body>) -> HttpRequest<Body> {
        let peer: std::net::SocketAddr = "192.0.2.50:1".parse().unwrap();
        req.extensions_mut().insert(ConnectInfo(peer));
        req
    }

    const CAPS: RouteTransportCapabilities = RouteTransportCapabilities {
        request_types: &[],
        response_types: &[],
        default_response_type: "",
        supports_sequence_response: false,
    };

    const ROUTES: &[RouteTransportDescriptor] = &[
        RouteTransportDescriptor {
            name: "createPayment",
            method: "POST",
            path: "/$procs/createPayment",
            capabilities: CAPS,
            rate_limited_by_default: false,
        },
        RouteTransportDescriptor {
            name: "Widget",
            method: "GET",
            path: "/widgets/{id}",
            capabilities: CAPS,
            rate_limited_by_default: true,
        },
    ];

    async fn ok() -> &'static str {
        "ok"
    }

    fn app() -> Router {
        Router::new()
            .route("/$procs/createPayment", post(ok))
            .route("/widgets/{id}", get(ok))
            .route_layer(
                RateLimitLayer::new(
                    std::sync::Arc::new(InMemoryRateLimitStore::default()),
                    RateLimitConfig::new(1, 0.001),
                )
                .with_should_rate_limit_fn(build_rest_ops_filter(ROUTES)),
            )
    }

    /// AC1/AC2 mirrored for REST: an exempt route survives past its
    /// burst, an un-annotated route (with a path param, proving
    /// `MatchedPath` — not the concrete request path — is what's
    /// compared) is throttled.
    #[tokio::test]
    async fn route_layer_exempts_annotated_route_and_throttles_others() {
        let router = app();

        for _ in 0..3 {
            let resp = router
                .clone()
                .oneshot(
                    HttpRequest::post("/$procs/createPayment")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::OK,
                "@no_rate_limit route should never be throttled"
            );
        }

        let first = router
            .clone()
            .oneshot(with_peer(
                HttpRequest::get("/widgets/42").body(Body::empty()).unwrap(),
            ))
            .await
            .unwrap();
        assert_eq!(first.status(), StatusCode::OK, "first request within burst");

        let second = router
            .clone()
            .oneshot(with_peer(
                HttpRequest::get("/widgets/7").body(Body::empty()).unwrap(),
            ))
            .await
            .unwrap();
        assert_eq!(
            second.status(),
            StatusCode::TOO_MANY_REQUESTS,
            "un-annotated route is throttled regardless of the concrete id in the path, \
             proving the comparison is against the matched route pattern"
        );
    }

    /// `Router::layer` (unlike `route_layer`) also wraps the fallback
    /// (404) service, so a request that matches no route still runs
    /// through the filter, finds no `MatchedPath`, and fails closed —
    /// consuming budget even for a 404. This proves the filter never
    /// fails open on an unmatched path.
    #[tokio::test]
    async fn plain_layer_fails_closed_and_throttles_unmatched_paths() {
        let router = Router::new()
            .route("/$procs/createPayment", post(ok))
            .layer(
                RateLimitLayer::new(
                    std::sync::Arc::new(InMemoryRateLimitStore::default()),
                    RateLimitConfig::new(1, 0.001),
                )
                .with_should_rate_limit_fn(build_rest_ops_filter(ROUTES)),
            );

        let first = router
            .clone()
            .oneshot(with_peer(
                HttpRequest::get("/does/not/exist")
                    .body(Body::empty())
                    .unwrap(),
            ))
            .await
            .unwrap();
        assert_eq!(
            first.status(),
            StatusCode::NOT_FOUND,
            "first 404 within burst still reaches the fallback"
        );

        let second = router
            .clone()
            .oneshot(with_peer(
                HttpRequest::get("/does/not/exist")
                    .body(Body::empty())
                    .unwrap(),
            ))
            .await
            .unwrap();
        assert_eq!(
            second.status(),
            StatusCode::TOO_MANY_REQUESTS,
            "Router::layer wraps the fallback too, so an unmatched path has no \
             MatchedPath, fails closed, and is throttled just like any other route"
        );
    }

    /// `Router::route_layer`'s counterpart: it does *not* wrap the
    /// fallback, so unmatched paths bypass the rate limiter (and this
    /// filter) entirely — every 404 stays a 404, never a 429, and never
    /// consumes budget.
    #[tokio::test]
    async fn route_layer_does_not_throttle_unmatched_paths() {
        let router = Router::new()
            .route("/$procs/createPayment", post(ok))
            .route_layer(
                RateLimitLayer::new(
                    std::sync::Arc::new(InMemoryRateLimitStore::default()),
                    RateLimitConfig::new(1, 0.001),
                )
                .with_should_rate_limit_fn(build_rest_ops_filter(ROUTES)),
            );

        for _ in 0..5 {
            let resp = router
                .clone()
                .oneshot(
                    HttpRequest::get("/does/not/exist")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::NOT_FOUND,
                "route_layer skips the fallback entirely, so repeated unmatched \
                 requests are always 404, never 429"
            );
        }
    }
}
