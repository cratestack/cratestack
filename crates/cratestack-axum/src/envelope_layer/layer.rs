//! [`EnvelopeLayer`] and the configuration it shares with its services.

use std::fmt;
use std::sync::Arc;

use cratestack_core::AcceptedContracts;
use tower::Layer;

use super::builder::{EnvelopeLayerBuilder, Transport};
use super::mode::EnvelopePolicy;
use super::principal::{PrincipalMapper, ThumbprintPrincipal};
use super::resolver::BindingResolver;
use super::seal_policy::{AcceptNamesEnvelope, ResponseSealPolicy};
use super::server_envelope::ServerEnvelope;
use super::service::EnvelopeService;
use super::{DEFAULT_MAX_BODY_BYTES, DEFAULT_MAX_CONTRACT_TRIALS};

pub(super) struct Config {
    pub(super) envelope: Arc<dyn ServerEnvelope>,
    pub(super) policy: Box<dyn EnvelopePolicy>,
    pub(super) resolver: Box<dyn BindingResolver>,
    pub(super) principal: Box<dyn PrincipalMapper>,
    pub(super) seal_policy: Box<dyn ResponseSealPolicy>,
    pub(super) audience: String,
    /// Per op key, the digests a request may bind (`contract.rs`).
    pub(super) contracts: AcceptedContracts,
    pub(super) max_contract_trials: usize,
    pub(super) max_body_bytes: usize,
    /// Normalised (`""` or `"/api"`), for the allow-list.
    pub(super) mount_prefix: String,
    /// Route templates, relative to the mount prefix, that may be matched
    /// without resolving (decision S2).
    pub(super) allow_unresolved: Vec<String>,
}

/// Opens signed requests and seals responses for a generated router; see
/// the [module docs](crate::envelope_layer) for placement and guarantees.
/// Build it with [`EnvelopeLayer::builder`], or with the generated
/// `cratestack_schema::axum::envelope_layer`. Cheap to clone.
#[derive(Clone)]
pub struct EnvelopeLayer {
    pub(super) config: Arc<Config>,
}

impl EnvelopeLayer {
    /// Start a layer for `envelope` (a `cratestack_cose::CoseEnvelope`
    /// built with `CoseEnvelope::server`, or your own [`ServerEnvelope`]).
    ///
    /// - `audience`: this service's configured logical id, which every
    ///   binding carries. Not the `Host` header, and distinct from the
    ///   audience this service seals its own outbound requests for.
    /// - `contracts`: the generated `cratestack_schema::ACCEPTED_CONTRACTS`,
    ///   per op, the op-contract digests a request may bind (binding
    ///   version 2, cratestack#1123). Which one a request used is told by
    ///   its `Cratestack-Contract` header (see [`EnvelopeLayerBuilder::max_contract_trials`]).
    ///
    /// Then name the transport ([`EnvelopeLayerBuilder::rest`],
    /// [`EnvelopeLayerBuilder::rpc`] or a custom
    /// [`EnvelopeLayerBuilder::binding_resolver`]) and the
    /// [`EnvelopeLayerBuilder::policy`]; neither has a default. The
    /// generated `envelope_layer(envelope, policy, audience)` does both
    /// for the schema's own transport.
    pub fn builder(
        envelope: impl ServerEnvelope,
        audience: impl Into<String>,
        contracts: AcceptedContracts,
    ) -> EnvelopeLayerBuilder {
        EnvelopeLayerBuilder {
            envelope: Arc::new(envelope),
            audience: audience.into(),
            contracts,
            max_contract_trials: DEFAULT_MAX_CONTRACT_TRIALS,
            policy: None,
            transport: Transport::None,
            transport_prefix: None,
            mount_prefix: None,
            unresolved_mode: None,
            allow_unresolved: Vec::new(),
            principal: Box::new(ThumbprintPrincipal::new()),
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
            .field("media_type", &self.config.envelope.media_type())
            .field("mount_prefix", &self.config.mount_prefix)
            .field("allow_unresolved", &self.config.allow_unresolved)
            .field("max_body_bytes", &self.config.max_body_bytes)
            .finish_non_exhaustive()
    }
}
