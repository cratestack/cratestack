//! [`EnvelopeMode`] and the per-op [`EnvelopePolicy`] that picks it.

use super::policy_request::PolicyRequest;

/// How the layer treats one op's traffic (ADR 0006 §2).
///
/// `#[non_exhaustive]` (API-review decision, 2026-09-26): a later mode
/// (say, "signed requests only, unsigned responses") must not be a breaking
/// change for a policy that matches on this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum EnvelopeMode {
    /// Every request must be signed, `GET`, `HEAD` and `DELETE` included
    /// (they seal an empty payload; decision D3). An unsigned one is refused
    /// with the unsigned `401`. Every response is sealed.
    ///
    /// A signed `HEAD` still sends its COSE message as a request body,
    /// which RFC 9110 §9.3.2 gives no semantics: hyper's server and
    /// `reqwest` pass it through over HTTP/1.1, but an intermediary may
    /// drop it (the request then fails verification, `401`) or refuse the
    /// request. Prefer `GET` under `Required`.
    Required,
    /// A signed request is opened and its response is always sealed, as
    /// under `Required` (decision S3). An unsigned one runs as before; its
    /// response is sealed only when it carries a valid `Cratestack-Nonce`
    /// and the [`super::ResponseSealPolicy`] asks for it (D10). A signed
    /// request that fails verification is still the `401`, never downgraded
    /// to unsigned.
    Optional,
    /// Plain traffic passes untouched. A request with a COSE body is still
    /// refused (`415`) rather than forwarded unverified.
    Off,
}

impl EnvelopeMode {
    /// The stricter of two modes: `Required` over `Optional` over `Off`.
    /// How a `/rpc/batch` call's frames combine (decision B1).
    pub(super) fn strictest(self, other: Self) -> Self {
        let rank = |mode: Self| match mode {
            Self::Required => 2,
            Self::Optional => 1,
            Self::Off => 0,
        };
        if rank(other) > rank(self) {
            other
        } else {
            self
        }
    }
}

/// Picks the [`EnvelopeMode`] for a request, from the op it addresses.
///
/// [`EnvelopeMode`] implements it (one mode for every op), and so does any
/// `Fn(&PolicyRequest<'_>) -> EnvelopeMode`, so a per-op policy is a
/// closure:
///
/// ```
/// use cratestack_axum::envelope_layer::{EnvelopeMode, PolicyRequest};
///
/// let policy = |request: &PolicyRequest<'_>| {
///     if request.is_subscription() { EnvelopeMode::Optional } else { EnvelopeMode::Required }
/// };
/// # let _ = policy;
/// ```
///
/// There is no default: the layer's builder refuses to build without one
/// (decision D9). A policy sees no header ([`PolicyRequest`]), so no
/// request can talk it into a weaker mode.
///
/// **What the layer enforces whatever this returns:** a request with a
/// COSE body is opened (or, under `Off`, refused) regardless; under
/// `Required` no unsigned request reaches the router and no response leaves
/// unsealed; a `/rpc/batch` call runs under the strictest mode of `batch`
/// and every frame's op (decision B1). A policy can only choose how strict
/// to be for plain traffic, never make the layer accept an unverified
/// envelope.
pub trait EnvelopePolicy: Send + Sync + 'static {
    /// The mode for this op.
    fn mode(&self, request: &PolicyRequest<'_>) -> EnvelopeMode;

    /// The mode for traffic the layer cannot attribute to one op: a route
    /// the router matched but the resolver cannot bind (a wrong mount
    /// prefix, a hand-written route; decision S2), or a `/rpc/batch` body
    /// whose frames it cannot read (B1). Under `Required` the first fails
    /// closed with a `500` unless the route is allow-listed
    /// ([`super::EnvelopeLayerBuilder::allow_unresolved`]), and the second
    /// is refused. Under a non-`Required` answer, both pass through, and so
    /// does a method the schema does not generate on a generated path
    /// (otherwise the layer's `405`), unsigned.
    ///
    /// The default is `Required`, which fails closed: a per-op closure
    /// cannot say what it would have answered for an op nobody can name.
    /// An [`EnvelopeMode`] used as the policy answers itself. For a
    /// closure, [`super::EnvelopeLayerBuilder::unresolved_mode`] replaces
    /// it explicitly, without writing a type.
    fn unresolved_mode(&self) -> EnvelopeMode {
        EnvelopeMode::Required
    }
}

impl EnvelopePolicy for EnvelopeMode {
    fn mode(&self, _request: &PolicyRequest<'_>) -> EnvelopeMode {
        *self
    }

    fn unresolved_mode(&self) -> EnvelopeMode {
        *self
    }
}

impl<F> EnvelopePolicy for F
where
    F: Fn(&PolicyRequest<'_>) -> EnvelopeMode + Send + Sync + 'static,
{
    fn mode(&self, request: &PolicyRequest<'_>) -> EnvelopeMode {
        self(request)
    }
}

/// A policy whose `unresolved_mode` the builder replaced
/// ([`super::EnvelopeLayerBuilder::unresolved_mode`]).
pub(super) struct WithUnresolved {
    pub(super) policy: Box<dyn EnvelopePolicy>,
    pub(super) unresolved: EnvelopeMode,
}

impl EnvelopePolicy for WithUnresolved {
    fn mode(&self, request: &PolicyRequest<'_>) -> EnvelopeMode {
        self.policy.mode(request)
    }

    fn unresolved_mode(&self) -> EnvelopeMode {
        self.unresolved
    }
}

#[cfg(test)]
mod tests {
    use super::EnvelopeMode::{Off, Optional, Required};

    #[test]
    fn strictest_is_symmetric_and_ranks_required_first() {
        for (a, b, expected) in [
            (Off, Optional, Optional),
            (Optional, Required, Required),
            (Off, Required, Required),
            (Off, Off, Off),
        ] {
            assert_eq!(a.strictest(b), expected);
            assert_eq!(b.strictest(a), expected);
        }
    }
}
