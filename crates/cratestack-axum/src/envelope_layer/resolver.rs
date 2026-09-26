//! [`BindingResolver`]: which generated op a request addresses, as the
//! `route` and `path_params` of its binding (ADR 0006 §4). The built-in
//! resolvers are in `resolver_rest` and `resolver_rpc`.

use std::borrow::Cow;

use http::Method;

/// What a resolver sees of a request: the method, the raw path, the route
/// template axum matched (`None` when no route matched, or under
/// `nest_service`), and the matched path parameters, percent-decoded, in the
/// order axum reports them (a parameterised mount prefix's first).
#[derive(Debug, Clone, Copy)]
pub struct RouteRequest<'a> {
    method: &'a Method,
    path: &'a str,
    matched_path: Option<&'a str>,
    path_params: &'a [(String, String)],
}

impl<'a> RouteRequest<'a> {
    /// A request as the layer describes it. Public so a custom resolver can
    /// be unit-tested; the layer builds its own from the request.
    pub fn new(
        method: &'a Method,
        path: &'a str,
        matched_path: Option<&'a str>,
        path_params: &'a [(String, String)],
    ) -> Self {
        Self {
            method,
            path,
            matched_path,
            path_params,
        }
    }

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

/// The route an RPC subscription is bound under: `subscribe/<op id>`.
const SUBSCRIBE_ROUTE_PREFIX: &str = "subscribe/";

impl ResolvedRoute {
    /// `route` as the client binds it: the RPC op id (`batch` for
    /// `/rpc/batch`, `subscribe/<op id>` for a subscription), or the REST
    /// route template as the schema declares it, without the mount prefix.
    /// `path_params` in the order they are bound.
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

    /// The op an [`super::EnvelopePolicy`] is asked about: the route, or for
    /// a subscription the bare op id after `subscribe/` (decision B1).
    pub fn op(&self) -> &str {
        self.route
            .strip_prefix(SUBSCRIBE_ROUTE_PREFIX)
            .unwrap_or(&self.route)
    }

    /// Bound as `subscribe/<op id>`: an RPC subscription, which streams and
    /// so can never answer a signed request (the layer refuses it with a
    /// sealed `406` before the handler runs).
    pub fn is_subscription(&self) -> bool {
        self.route.starts_with(SUBSCRIBE_ROUTE_PREFIX)
    }

    /// Bound as `batch`: the whole `/rpc/batch` call (decision D11), whose
    /// frames the layer reads to apply the policy to each (B1). No REST
    /// template (they start with `/`) and no unary op id (the RPC resolver
    /// never binds one without a `.`, B2) can be `batch`.
    pub fn is_batch(&self) -> bool {
        self.route == "batch"
    }
}

/// A [`BindingResolver`]'s answer.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Resolution {
    /// A generated op: the request is opened, verified, or refused
    /// according to the policy.
    Op(ResolvedRoute),
    /// A route the resolver recognises and knows is not an op, such as an
    /// RPC op id that no generated op can have (`/rpc/%62atch`, decision
    /// B2). A plain request passes through (the router answers it, a
    /// `404`), a COSE one is refused with the unsigned `415`.
    NotAnOp,
    /// A generated route, but not for this method: the router answers
    /// `405` with these methods in `Allow`. Passes through, unless the
    /// policy's `unresolved_mode` is `Required` and the route is not
    /// allow-listed: then the layer answers the `405` itself, so a
    /// hand-written handler for another method on a generated path cannot
    /// run unsigned (decision S2).
    MethodNotAllowed(Vec<Method>),
    /// A route the resolver does not know. With no matched route (a `404`)
    /// it passes through; with one, under a `Required` `unresolved_mode`
    /// and not allow-listed, the layer fails closed with a `500` (S2).
    Unresolved,
}

/// Resolves the op a request addresses.
///
/// Use [`super::RestBindingResolver`] or [`super::RpcBindingResolver`]
/// unless the router is mounted in a way they cannot see through.
///
/// **What the layer enforces whatever this returns:** it is called exactly
/// once per request, and the request and the response are bound with the
/// same [`ResolvedRoute`], so a resolver cannot make the two disagree. A
/// wrong answer can only make verification fail (the client bound something
/// else), fail closed under `Required` ([`Resolution::Unresolved`]), or, by
/// returning [`Resolution::NotAnOp`] for a real op, let *plain* traffic to
/// it through; it can never make an unverified envelope pass.
pub trait BindingResolver: Send + Sync + 'static {
    /// The op `request` addresses, if it is one.
    fn resolve(&self, request: &RouteRequest<'_>) -> Resolution;
}
