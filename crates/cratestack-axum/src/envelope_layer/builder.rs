//! [`EnvelopeLayerBuilder`]: nothing is checked until `build`.

use std::sync::Arc;

use cratestack_core::{CratestackError, RouteTransportDescriptor};
use http::HeaderValue;

use super::layer::{Config, EnvelopeLayer};
use super::media;
use super::mode::EnvelopePolicy;
use super::principal::PrincipalMapper;
use super::resolver::BindingResolver;
use super::resolver_rest::RestBindingResolver;
use super::resolver_rpc::RpcBindingResolver;
use super::seal_policy::ResponseSealPolicy;
use super::server_envelope::ServerEnvelope;
use crate::idempotency::mount_prefix;

pub(super) enum Transport {
    None,
    Rest(&'static [RouteTransportDescriptor]),
    Rpc,
    Custom(Box<dyn BindingResolver>),
}

/// Configuration for an [`EnvelopeLayer`]. Nothing is checked until
/// [`build`](Self::build).
pub struct EnvelopeLayerBuilder {
    pub(super) envelope: Arc<dyn ServerEnvelope>,
    pub(super) audience: String,
    pub(super) schema_sha: [u8; 32],
    pub(super) policy: Option<Box<dyn EnvelopePolicy>>,
    pub(super) transport: Transport,
    pub(super) mount_prefix: Option<String>,
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
        self.mount_prefix = Some(prefix.to_owned());
        self
    }

    /// A `transport rpc` router mounted at `prefix` (`""` at the root).
    pub fn rpc(mut self, prefix: &str) -> Self {
        self.transport = Transport::Rpc;
        self.mount_prefix = Some(prefix.to_owned());
        self
    }

    /// A custom [`BindingResolver`], instead of [`rest`](Self::rest) or
    /// [`rpc`](Self::rpc).
    pub fn binding_resolver(mut self, resolver: impl BindingResolver) -> Self {
        self.transport = Transport::Custom(Box::new(resolver));
        self
    }

    /// Where the router is mounted (`"/api"` for `Router::nest("/api",
    /// router)`), replacing the prefix given to [`rest`](Self::rest) or
    /// [`rpc`](Self::rpc): for the generated `envelope_layer`, which
    /// assumes the root. Also what [`allow_unresolved`](Self::allow_unresolved)
    /// templates are relative to.
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

    /// Replace [`super::ThumbprintPrincipal`] (`cose:<hex thumbprint>`).
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

    /// Check the configuration: `CratestackError::Validation` when the
    /// audience is empty (it would bind no recipient), the policy or the
    /// transport is missing, the REST route table is empty (a `rest(..)`
    /// given another schema's, or an RPC schema's, empty table would bind
    /// nothing), the body limit is zero, an allow-listed template does not
    /// start with `/`, or the envelope's media type is not a valid header
    /// value naming `application/cose` or a type the envelope claims.
    pub fn build(self) -> Result<EnvelopeLayer, CratestackError> {
        if self.audience.is_empty() {
            return Err(invalid("the envelope layer's audience must not be empty"));
        }
        let policy = self
            .policy
            .ok_or_else(|| invalid("the envelope layer has no policy (decision D9: no default)"))?;
        let prefix = self.mount_prefix.as_deref().unwrap_or("");
        let resolver: Box<dyn BindingResolver> = match self.transport {
            Transport::None => {
                return Err(invalid(
                    "the envelope layer needs a transport: rest(..), rpc(..) or binding_resolver(..)",
                ));
            }
            Transport::Rest([]) => {
                return Err(invalid(
                    "rest(..) was given an empty route table: pass the generated \
                     ROUTE_TRANSPORTS of a REST schema, or use rpc(..) for `transport rpc`",
                ));
            }
            Transport::Rest(routes) => Box::new(RestBindingResolver::new(prefix, routes)),
            Transport::Rpc => Box::new(RpcBindingResolver::new(prefix)),
            Transport::Custom(resolver) => resolver,
        };
        if self.max_body_bytes == 0 {
            return Err(invalid("the envelope layer's body limit must not be zero"));
        }
        if let Some(bad) = self.allow_unresolved.iter().find(|t| !t.starts_with('/')) {
            return Err(invalid(&format!(
                "allow_unresolved({bad:?}): a route template starts with '/'"
            )));
        }
        let media_type = self.envelope.media_type();
        if HeaderValue::from_str(media_type).is_err()
            || !media::is_envelope_media_type(media_type, &*self.envelope)
        {
            return Err(invalid(
                "the envelope's media type must be a valid header value naming \
                 application/cose or a type the envelope claims (is_envelope_content_type)",
            ));
        }
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
                mount_prefix: mount_prefix::normalize(prefix),
                allow_unresolved: self.allow_unresolved,
            }),
        })
    }
}

fn invalid(message: &str) -> CratestackError {
    CratestackError::Validation(message.to_owned())
}
