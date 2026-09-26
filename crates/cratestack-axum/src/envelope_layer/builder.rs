//! [`EnvelopeLayerBuilder`]: nothing is checked until `build` (in
//! `build.rs`), and the calls commute: the order they are made in never
//! changes the layer.

use std::sync::Arc;

use cratestack_core::RouteTransportDescriptor;

use super::mode::{EnvelopeMode, EnvelopePolicy};
use super::principal::PrincipalMapper;
use super::resolver::BindingResolver;
use super::seal_policy::ResponseSealPolicy;
use super::server_envelope::ServerEnvelope;

pub(super) enum Transport {
    None,
    Rest(&'static [RouteTransportDescriptor]),
    Rpc,
    Custom(Box<dyn BindingResolver>),
}

/// Configuration for an [`super::EnvelopeLayer`]. Nothing is checked until
/// [`build`](Self::build), and the calls may come in any order.
pub struct EnvelopeLayerBuilder {
    pub(super) envelope: Arc<dyn ServerEnvelope>,
    pub(super) audience: String,
    pub(super) schema_sha: [u8; 32],
    pub(super) policy: Option<Box<dyn EnvelopePolicy>>,
    pub(super) transport: Transport,
    /// The prefix given to `rest(..)` / `rpc(..)`.
    pub(super) transport_prefix: Option<String>,
    /// The prefix given to `mount_prefix(..)`, which wins over the
    /// transport's in any order (second-review nit).
    pub(super) mount_prefix: Option<String>,
    pub(super) unresolved_mode: Option<EnvelopeMode>,
    pub(super) allow_unresolved: Vec<String>,
    pub(super) principal: Box<dyn PrincipalMapper>,
    pub(super) seal_policy: Box<dyn ResponseSealPolicy>,
    pub(super) max_body_bytes: usize,
}

impl EnvelopeLayerBuilder {
    /// Which mode each op runs in: an [`super::EnvelopeMode`] for all of
    /// them, or a closure per op. Required (decision D9).
    pub fn policy(mut self, policy: impl EnvelopePolicy) -> Self {
        self.policy = Some(Box::new(policy));
        self
    }

    /// A REST router mounted at `prefix` (`""` at the root), over the
    /// generated `cratestack_schema::axum::ROUTE_TRANSPORTS`, which must not
    /// be empty.
    pub fn rest(mut self, prefix: &str, routes: &'static [RouteTransportDescriptor]) -> Self {
        self.transport = Transport::Rest(routes);
        self.transport_prefix = Some(prefix.to_owned());
        self
    }

    /// A `transport rpc` router mounted at `prefix` (`""` at the root).
    pub fn rpc(mut self, prefix: &str) -> Self {
        self.transport = Transport::Rpc;
        self.transport_prefix = Some(prefix.to_owned());
        self
    }

    /// A custom [`BindingResolver`], instead of [`rest`](Self::rest) or
    /// [`rpc`](Self::rpc).
    pub fn binding_resolver(mut self, resolver: impl BindingResolver) -> Self {
        self.transport = Transport::Custom(Box::new(resolver));
        self
    }

    /// Where the router is mounted (`"/api"` for `Router::nest("/api",
    /// router)`). It wins over the prefix given to [`rest`](Self::rest) or
    /// [`rpc`](Self::rpc), whichever order the calls come in: the
    /// generated `envelope_layer` passes the root to those, so this is how
    /// its layer is told about a mount. Also what
    /// [`allow_unresolved`](Self::allow_unresolved) templates are relative
    /// to.
    pub fn mount_prefix(mut self, prefix: &str) -> Self {
        self.mount_prefix = Some(prefix.to_owned());
        self
    }

    /// Route templates (as given to `Router::route`, relative to the mount
    /// prefix, e.g. `"/health"`) that the router may match without the
    /// resolver knowing them: hand-written routes merged into the generated
    /// router before the layer. Plain traffic to them passes through
    /// unsigned. Any other matched route the resolver cannot bind fails
    /// closed with a `500` when the policy's `unresolved_mode` is
    /// `Required` (decision S2), which is what a wrong or missing mount
    /// prefix looks like. A COSE body to an allow-listed route is still
    /// refused (`415`).
    pub fn allow_unresolved<I, T>(mut self, templates: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<String>,
    {
        self.allow_unresolved
            .extend(templates.into_iter().map(Into::into));
        self
    }

    /// Replace the policy's
    /// [`unresolved_mode`](super::EnvelopePolicy::unresolved_mode) with
    /// `mode`, whatever the policy answers (second-review nit). An explicit
    /// opt-in, mostly for a closure policy, whose `unresolved_mode` is
    /// always `Required`: with `Optional` or `Off` here, a matched route the
    /// resolver cannot bind passes through plain (warned about once per
    /// process) instead of failing closed with the `500`, and so does
    /// every route when the mount prefix is wrong. Prefer listing
    /// hand-written routes with [`allow_unresolved`](Self::allow_unresolved),
    /// which keeps a misconfiguration loud. The ops' own modes are
    /// unchanged; a `/rpc/batch` whose frames cannot be read uses this
    /// mode too.
    pub fn unresolved_mode(mut self, mode: EnvelopeMode) -> Self {
        self.unresolved_mode = Some(mode);
        self
    }

    /// Replace [`super::ThumbprintPrincipal`] (`cose:<hex thumbprint>`;
    /// `ThumbprintPrincipal::with_prefix(..)` keeps the thumbprint and
    /// changes the prefix).
    pub fn principal_mapper(mut self, mapper: impl PrincipalMapper) -> Self {
        self.principal = Box::new(mapper);
        self
    }

    /// Replace [`super::AcceptNamesEnvelope`] (decision D10).
    pub fn response_seal_policy(mut self, policy: impl ResponseSealPolicy) -> Self {
        self.seal_policy = Box::new(policy);
        self
    }

    /// The largest request body the layer buffers. Default
    /// [`super::DEFAULT_MAX_BODY_BYTES`]; keep it a little above the
    /// router's own body limit, which applies to the payload after opening.
    pub fn max_body_bytes(mut self, limit: usize) -> Self {
        self.max_body_bytes = limit;
        self
    }
}
