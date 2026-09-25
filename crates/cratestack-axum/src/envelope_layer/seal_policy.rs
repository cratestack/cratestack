//! [`ResponseSealPolicy`]: under `Optional`, whether an unsigned request's
//! response is sealed (maintainer decision D10).

use http::{HeaderMap, Method};

use super::resolver::ResolvedRoute;

/// An unsigned request under `Optional` that carries a valid
/// `Cratestack-Nonce`, as a [`ResponseSealPolicy`] sees it.
#[derive(Debug)]
pub struct UnsignedRequest<'a> {
    pub(super) method: &'a Method,
    pub(super) route: &'a ResolvedRoute,
    pub(super) headers: &'a HeaderMap,
    pub(super) accept_names_envelope: bool,
}

impl<'a> UnsignedRequest<'a> {
    /// The request method.
    pub fn method(&self) -> &'a Method {
        self.method
    }

    /// The op it addresses.
    pub fn route(&self) -> &'a ResolvedRoute {
        self.route
    }

    /// Its headers.
    pub fn headers(&self) -> &'a HeaderMap {
        self.headers
    }

    /// Whether an `Accept` entry with a non-zero weight names
    /// `application/cose` or the envelope's own media type.
    pub fn accept_names_envelope(&self) -> bool {
        self.accept_names_envelope
    }
}

/// Decides whether the response to an unsigned request is sealed, under
/// `Optional`. The default is [`AcceptNamesEnvelope`] (D10).
///
/// **What the layer enforces whatever this returns:** it is asked only
/// under `Optional`, only about an unsigned request, and only when that
/// request carries a well-formed `Cratestack-Nonce` (without one the
/// response could not be bound to this request, and a cached one could be
/// replayed for the next). A signed request's response is always sealed,
/// and under `Required` every response is: neither is this policy's call.
pub trait ResponseSealPolicy: Send + Sync + 'static {
    /// `true` to seal this request's response.
    fn seal_unsigned(&self, request: &UnsignedRequest<'_>) -> bool;
}

/// Seal when the request's `Accept` names `application/cose` (D10).
#[derive(Debug, Clone, Copy, Default)]
pub struct AcceptNamesEnvelope;

impl ResponseSealPolicy for AcceptNamesEnvelope {
    fn seal_unsigned(&self, request: &UnsignedRequest<'_>) -> bool {
        request.accept_names_envelope
    }
}

impl<F> ResponseSealPolicy for F
where
    F: Fn(&UnsignedRequest<'_>) -> bool + Send + Sync + 'static,
{
    fn seal_unsigned(&self, request: &UnsignedRequest<'_>) -> bool {
        self(request)
    }
}
