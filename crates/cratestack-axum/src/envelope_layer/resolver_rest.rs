//! [`RestBindingResolver`]: the generated REST router's ops.

use std::str::FromStr;

use cratestack_core::RouteTransportDescriptor;
use http::Method;

use super::resolver::{BindingResolver, Resolution, ResolvedRoute, RouteRequest};
use crate::idempotency::mount_prefix;

/// REST: the `RouteTransportDescriptor` matching the method and the matched
/// template (mount prefix stripped), bound by its declared path.
///
/// Path parameters are every matched parameter in axum's order, including
/// those of a parameterised mount (`nest("/t/{tenant}", ..)`), which come
/// first: binding only the template's would let a signed request for tenant
/// `a` be replayed at tenant `b`. A client of such a mount binds those
/// values too.
///
/// The request's canonical query is bound as well (the layer's own input,
/// not the resolver's): see `cratestack_core::canonical_query` for its
/// order semantics.
#[derive(Debug, Clone)]
pub struct RestBindingResolver {
    prefix: String,
    routes: &'static [RouteTransportDescriptor],
}

impl RestBindingResolver {
    /// For a router mounted at `prefix` (`""` at the root; `"/api"` for
    /// `Router::nest("/api", ..)`), over the generated `ROUTE_TRANSPORTS`.
    pub fn new(prefix: &str, routes: &'static [RouteTransportDescriptor]) -> Self {
        Self {
            prefix: mount_prefix::normalize(prefix),
            routes,
        }
    }
}

impl BindingResolver for RestBindingResolver {
    fn resolve(&self, request: &RouteRequest<'_>) -> Resolution {
        let Some(path) = request
            .matched_path()
            .and_then(|matched| mount_prefix::strip(matched, &self.prefix))
        else {
            return Resolution::Unresolved;
        };
        // axum answers `HEAD` with the `GET` route, which is the only one
        // the descriptors list; without this a `HEAD` would resolve to
        // nothing and pass unsigned under `Required` (D3 signs it too). The
        // binding keeps the real method, `HEAD`.
        let method = match request.method().as_str() {
            "HEAD" => "GET",
            method => method,
        };
        let mut on_path = self.routes.iter().filter(|route| route.path == path);
        if let Some(route) = on_path.clone().find(|route| route.method == method) {
            let params = request
                .path_params()
                .iter()
                .map(|(_, value)| value.clone())
                .collect();
            return Resolution::Op(ResolvedRoute::new(route.path, params));
        }
        // A generated path, another method: the router's `405` (S2 no
        // longer warns about it as a misconfiguration).
        let allow: Vec<Method> = on_path
            .by_ref()
            .filter_map(|route| Method::from_str(route.method).ok())
            .collect();
        if allow.is_empty() {
            Resolution::Unresolved
        } else {
            Resolution::MethodNotAllowed(allow)
        }
    }
}
