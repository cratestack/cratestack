//! [`BindingResolver`]: which generated op a request addresses, as the
//! `route` and `path_params` of its binding (ADR 0006 §4).

use std::borrow::Cow;

use cratestack_core::RouteTransportDescriptor;
use cratestack_core::rpc::{RPC_BATCH_PATH, RPC_SUBSCRIBE_PATH, RPC_UNARY_PATH};
use http::Method;

use crate::idempotency::mount_prefix;

/// What a resolver sees of a request: the method, the raw path, the route
/// template axum matched (`None` when no route matched, or under
/// `nest_service`), and the matched path parameters, percent-decoded, in the
/// order axum reports them (a parameterised mount prefix's first).
#[derive(Debug, Clone, Copy)]
pub struct RouteRequest<'a> {
    pub(super) method: &'a Method,
    pub(super) path: &'a str,
    pub(super) matched_path: Option<&'a str>,
    pub(super) path_params: &'a [(String, String)],
}

impl<'a> RouteRequest<'a> {
    /// The request method.
    pub fn method(&self) -> &'a Method {
        self.method
    }

    /// The raw request path, mount prefix included.
    pub fn path(&self) -> &'a str {
        self.path
    }

    /// The route template axum matched, mount prefix included.
    pub fn matched_path(&self) -> Option<&'a str> {
        self.matched_path
    }

    /// `(name, value)` pairs, decoded.
    pub fn path_params(&self) -> &'a [(String, String)] {
        self.path_params
    }
}

/// A resolved op: the binding's `route` and `path_params`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRoute {
    route: Cow<'static, str>,
    path_params: Vec<String>,
}

impl ResolvedRoute {
    /// `route` as the client binds it: the RPC op id (`batch` for
    /// `/rpc/batch`), or the REST route template as the schema declares it,
    /// without the mount prefix. `path_params` in the order they are bound.
    pub fn new(route: impl Into<Cow<'static, str>>, path_params: Vec<String>) -> Self {
        Self {
            route: route.into(),
            path_params,
        }
    }

    /// The route.
    pub fn route(&self) -> &str {
        &self.route
    }

    /// The path parameter values.
    pub fn path_params(&self) -> &[String] {
        &self.path_params
    }
}

/// Resolves the op a request addresses. `None` means "not a generated op":
/// the layer lets a plain request through untouched (decision D6) and
/// refuses one with a COSE body.
///
/// Use [`RestBindingResolver`] or [`RpcBindingResolver`] unless the router
/// is mounted in a way they cannot see through.
///
/// **What the layer enforces whatever this returns:** it is called exactly
/// once per request, and the request and the response are bound with the
/// same [`ResolvedRoute`], so a resolver cannot make the two disagree. A
/// wrong answer can only make verification fail (the client bound something
/// else) or, by returning `None` for a real op, let *plain* traffic to it
/// through; it can never make an unverified envelope pass.
pub trait BindingResolver: Send + Sync + 'static {
    /// The op `request` addresses, if it is one.
    fn resolve(&self, request: &RouteRequest<'_>) -> Option<ResolvedRoute>;
}

/// REST: the `RouteTransportDescriptor` matching the method and the matched
/// template (mount prefix stripped), bound by its declared path.
///
/// Path parameters are every matched parameter in axum's order, including
/// those of a parameterised mount (`nest("/t/{tenant}", ..)`), which come
/// first: binding only the template's would let a signed request for tenant
/// `a` be replayed at tenant `b`. A client of such a mount binds those
/// values too.
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
    fn resolve(&self, request: &RouteRequest<'_>) -> Option<ResolvedRoute> {
        let path = mount_prefix::strip(request.matched_path?, &self.prefix)?;
        let method = request.method.as_str();
        let route = self
            .routes
            .iter()
            .find(|route| route.method == method && route.path == path)?;
        let params = request
            .path_params
            .iter()
            .map(|(_, value)| value.clone())
            .collect();
        Some(ResolvedRoute::new(route.path, params))
    }
}

/// RPC: the op id axum decoded from `/rpc/{op_id}`, `batch` for
/// `/rpc/batch` (decision D11), and `subscribe/<op id>` for a subscription.
/// The op id and the body digest bind the call, so path parameters are
/// empty (ADR 0006 §4), except under a parameterised mount
/// (`nest("/t/{tenant}", ..)`), whose values are bound for the same reason
/// as [`RestBindingResolver`]'s.
///
/// The op id is not looked up in `OPS`: an unknown one is bound anyway, so
/// the router's `404` for it is sealed like any other error.
#[derive(Debug, Clone)]
pub struct RpcBindingResolver {
    prefix: String,
}

impl RpcBindingResolver {
    /// For a router mounted at `prefix`, as for [`RestBindingResolver::new`].
    pub fn new(prefix: &str) -> Self {
        Self {
            prefix: mount_prefix::normalize(prefix),
        }
    }
}

impl BindingResolver for RpcBindingResolver {
    fn resolve(&self, request: &RouteRequest<'_>) -> Option<ResolvedRoute> {
        let template = mount_prefix::strip(request.matched_path?, &self.prefix)?;
        let values = |params: &[(String, String)]| -> Vec<String> {
            params.iter().map(|(_, value)| value.clone()).collect()
        };
        if template == RPC_BATCH_PATH {
            return Some(ResolvedRoute::new("batch", values(request.path_params)));
        }
        // The op id is the last parameter (a parameterised mount's come
        // first), bound as the router decoded it, which is what it runs.
        let (op_id, mount) = request.path_params.split_last()?;
        let op_id = &op_id.1;
        if template == RPC_UNARY_PATH {
            Some(ResolvedRoute::new(op_id.clone(), values(mount)))
        } else if template == RPC_SUBSCRIBE_PATH {
            Some(ResolvedRoute::new(
                format!("subscribe/{op_id}"),
                values(mount),
            ))
        } else {
            None
        }
    }
}
