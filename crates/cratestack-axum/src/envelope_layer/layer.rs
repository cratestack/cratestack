//! [`EnvelopeLayer`] and its builder.

use std::fmt;
use std::sync::Arc;

use cratestack_core::{CratestackError, RouteTransportDescriptor};
use http::HeaderValue;
use tower::Layer;

use super::DEFAULT_MAX_BODY_BYTES;
use super::mode::EnvelopePolicy;
use super::principal::{PrincipalMapper, ThumbprintPrincipal};
use super::resolver::{BindingResolver, RestBindingResolver, RpcBindingResolver};
use super::seal_policy::{AcceptNamesEnvelope, ResponseSealPolicy};
use super::server_envelope::ServerEnvelope;
use super::service::EnvelopeService;

pub(super) struct Config {
    pub(super) envelope: Arc<dyn ServerEnvelope>,
    pub(super) policy: Box<dyn EnvelopePolicy>,
    pub(super) resolver: Box<dyn BindingResolver>,
    pub(super) principal: Box<dyn PrincipalMapper>,
    pub(super) seal_policy: Box<dyn ResponseSealPolicy>,
    pub(super) audience: String,
    pub(super) schema_sha: [u8; 32],
    pub(super) max_body_bytes: usize,
    pub(super) media_type: HeaderValue,
}

/// Opens signed requests and seals responses for a generated router; see
/// the [module docs](super) for placement and guarantees. Build it with
/// [`EnvelopeLayer::builder`]. Cheap to clone.
#[derive(Clone)]
pub struct EnvelopeLayer {
    config: Arc<Config>,
}

impl EnvelopeLayer {
    /// Start a layer for `envelope` (a `cratestack_cose::CoseEnvelope`
    /// built with `CoseEnvelope::server`, or your own [`ServerEnvelope`]).
    ///
    /// - `audience`: this service's configured logical id, which every
    ///   binding carries. Not the `Host` header, and distinct from the
    ///   audience this service seals its own outbound requests for.
    /// - `schema_sha`: the generated `cratestack_schema::SCHEMA_SHA256_BYTES`.
    ///
    /// Then name the transport ([`EnvelopeLayerBuilder::rest`],
    /// [`EnvelopeLayerBuilder::rpc`] or a custom
    /// [`EnvelopeLayerBuilder::binding_resolver`]) and the
    /// [`EnvelopeLayerBuilder::policy`]; neither has a default.
    pub fn builder(
        envelope: impl ServerEnvelope,
        audience: impl Into<String>,
        schema_sha: [u8; 32],
    ) -> EnvelopeLayerBuilder {
        EnvelopeLayerBuilder {
            envelope: Arc::new(envelope),
            audience: audience.into(),
            schema_sha,
            policy: None,
            resolver: None,
            principal: Box::new(ThumbprintPrincipal),
            seal_policy: Box::new(AcceptNamesEnvelope),
            max_body_bytes: DEFAULT_MAX_BODY_BYTES,
        }
    }
}

impl<S> Layer<S> for EnvelopeLayer {
    type Service = EnvelopeService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        EnvelopeService {
            inner,
            config: self.config.clone(),
        }
    }
}

impl fmt::Debug for EnvelopeLayer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EnvelopeLayer")
            .field("audience", &self.config.audience)
            .field("media_type", &self.config.media_type)
            .field("max_body_bytes", &self.config.max_body_bytes)
            .finish_non_exhaustive()
    }
}

/// Configuration for an [`EnvelopeLayer`]. Nothing is checked until
/// [`build`](Self::build).
pub struct EnvelopeLayerBuilder {
    envelope: Arc<dyn ServerEnvelope>,
    audience: String,
    schema_sha: [u8; 32],
    policy: Option<Box<dyn EnvelopePolicy>>,
    resolver: Option<Box<dyn BindingResolver>>,
    principal: Box<dyn PrincipalMapper>,
    seal_policy: Box<dyn ResponseSealPolicy>,
    max_body_bytes: usize,
}

impl EnvelopeLayerBuilder {
    /// Which mode each op runs in: an [`super::EnvelopeMode`] for all of
    /// them, or a closure per op. Required (decision D9).
    pub fn policy(mut self, policy: impl EnvelopePolicy) -> Self {
        self.policy = Some(Box::new(policy));
        self
    }

    /// A REST router mounted at `prefix` (`""` at the root), over the
    /// generated `cratestack_schema::axum::ROUTE_TRANSPORTS`.
    pub fn rest(self, prefix: &str, routes: &'static [RouteTransportDescriptor]) -> Self {
        self.binding_resolver(RestBindingResolver::new(prefix, routes))
    }

    /// A `transport rpc` router mounted at `prefix` (`""` at the root).
    pub fn rpc(self, prefix: &str) -> Self {
        self.binding_resolver(RpcBindingResolver::new(prefix))
    }

    /// A custom [`BindingResolver`], instead of [`rest`](Self::rest) or
    /// [`rpc`](Self::rpc).
    pub fn binding_resolver(mut self, resolver: impl BindingResolver) -> Self {
        self.resolver = Some(Box::new(resolver));
        self
    }

    /// Replace [`ThumbprintPrincipal`] (`cose:<hex thumbprint>`).
    pub fn principal_mapper(mut self, mapper: impl PrincipalMapper) -> Self {
        self.principal = Box::new(mapper);
        self
    }

    /// Replace [`AcceptNamesEnvelope`] (decision D10).
    pub fn response_seal_policy(mut self, policy: impl ResponseSealPolicy) -> Self {
        self.seal_policy = Box::new(policy);
        self
    }

    /// The largest request body the layer buffers. Default
    /// [`DEFAULT_MAX_BODY_BYTES`]; keep it a little above the router's own
    /// body limit, which applies to the payload after opening.
    pub fn max_body_bytes(mut self, limit: usize) -> Self {
        self.max_body_bytes = limit;
        self
    }

    /// Check the configuration: `CratestackError::Validation` when the
    /// audience is empty (it would bind no recipient), the policy or the
    /// transport is missing, the body limit is zero, or the envelope's media
    /// type is not a valid header value.
    pub fn build(self) -> Result<EnvelopeLayer, CratestackError> {
        if self.audience.is_empty() {
            return Err(invalid("the envelope layer's audience must not be empty"));
        }
        let policy = self
            .policy
            .ok_or_else(|| invalid("the envelope layer has no policy (decision D9: no default)"))?;
        let resolver = self.resolver.ok_or_else(|| {
            invalid(
                "the envelope layer needs a transport: rest(..), rpc(..) or binding_resolver(..)",
            )
        })?;
        if self.max_body_bytes == 0 {
            return Err(invalid("the envelope layer's body limit must not be zero"));
        }
        let media_type = HeaderValue::from_str(self.envelope.media_type())
            .map_err(|_| invalid("the envelope's media type is not a valid header value"))?;
        Ok(EnvelopeLayer {
            config: Arc::new(Config {
                envelope: self.envelope,
                policy,
                resolver,
                principal: self.principal,
                seal_policy: self.seal_policy,
                audience: self.audience,
                schema_sha: self.schema_sha,
                max_body_bytes: self.max_body_bytes,
                media_type,
            }),
        })
    }
}

fn invalid(message: &str) -> CratestackError {
    CratestackError::Validation(message.to_owned())
}
