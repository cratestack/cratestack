//! [`ResponseSealPolicy`]: under `Optional`, whether an unsigned request's
//! response is sealed (maintainer decision D10).

use http::{HeaderMap, Method};

use super::resolver::ResolvedRoute;

/// An unsigned request under `Optional` that carries a valid
/// `Cratestack-Nonce`, as a [`ResponseSealPolicy`] sees it.
#[derive(Debug)]
pub struct UnsignedRequest<'a> {
    method: &'a Method,
    route: &'a ResolvedRoute,
    headers: &'a HeaderMap,
    accept_names_envelope: bool,
}

impl<'a> UnsignedRequest<'a> {
    /// A request as the layer describes it; `accept_names_envelope` is the
    /// layer's own reading of `headers`. Public so a policy can be
    /// unit-tested (unlike [`super::VerifiedRequest`], nothing here claims a
    /// verification happened).
    pub fn new(
        method: &'a Method,
        route: &'a ResolvedRoute,
        headers: &'a HeaderMap,
        accept_names_envelope: bool,
    ) -> Self {
        Self {
            method,
            route,
            headers,
            accept_names_envelope,
        }
    }

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
/// Such a seal binds the client's `Cratestack-Nonce` and the request
/// payload, and nothing about the caller: the request was not signed, so
/// the response proves only that this server answered this nonce and body,
/// not who asked. A client must not read it as an authenticated exchange.
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
