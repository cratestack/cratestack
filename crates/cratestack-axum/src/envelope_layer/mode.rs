//! [`EnvelopeMode`] and the per-op [`EnvelopePolicy`] that picks it.

use http::Method;

use super::resolver::ResolvedRoute;

/// How the layer treats one op's traffic (ADR 0006 §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnvelopeMode {
    /// Every request must be signed, `GET`, `HEAD` and `DELETE` included
    /// (they seal an empty payload; decision D3). An unsigned one is refused
    /// with the unsigned `401`. Every response is sealed.
    Required,
    /// A signed request is opened and its response sealed. An unsigned one
    /// runs as before; its response is sealed only when it carries a valid
    /// `Cratestack-Nonce` and the [`super::ResponseSealPolicy`] asks for it
    /// (D10). A signed request that fails verification is still the `401`,
    /// never downgraded to unsigned.
    Optional,
    /// Plain traffic passes untouched. A request with a COSE body is still
    /// refused (`415`) rather than forwarded unverified.
    Off,
}

/// Picks the [`EnvelopeMode`] for a request, from its method and the op the
/// [`super::BindingResolver`] resolved.
///
/// [`EnvelopeMode`] implements it (one mode for every op), and so does any
/// `Fn(&Method, &ResolvedRoute) -> EnvelopeMode`, so a per-op policy is a
/// closure:
///
/// ```text
/// |_method: &Method, route: &ResolvedRoute| {
///     if route.route().starts_with("subscribe/") { EnvelopeMode::Optional }
///     else { EnvelopeMode::Required }
/// }
/// ```
///
/// There is no default: the layer's builder refuses to build without one
/// (decision D9).
///
/// **What the layer enforces whatever this returns:** it is called once per
/// resolved request; a request with a COSE body is opened (or, under `Off`,
/// refused) regardless; under `Required` no unsigned request reaches the
/// router and no response leaves unsealed. A policy can only choose how
/// strict to be for plain traffic, never make the layer accept an
/// unverified envelope.
pub trait EnvelopePolicy: Send + Sync + 'static {
    /// The mode for this request.
    fn mode(&self, method: &Method, route: &ResolvedRoute) -> EnvelopeMode;
}

impl EnvelopePolicy for EnvelopeMode {
    fn mode(&self, _method: &Method, _route: &ResolvedRoute) -> EnvelopeMode {
        *self
    }
}

impl<F> EnvelopePolicy for F
where
    F: Fn(&Method, &ResolvedRoute) -> EnvelopeMode + Send + Sync + 'static,
{
    fn mode(&self, method: &Method, route: &ResolvedRoute) -> EnvelopeMode {
        self(method, route)
    }
}
