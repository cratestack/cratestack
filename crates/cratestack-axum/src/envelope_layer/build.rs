//! [`EnvelopeLayerBuilder::build`]: every check the configuration gets.

use std::sync::Arc;

use cratestack_core::CratestackError;
use http::HeaderValue;

use super::builder::{EnvelopeLayerBuilder, Transport};
use super::layer::{Config, EnvelopeLayer};
use super::media;
use super::mode::{EnvelopePolicy, WithUnresolved};
use super::resolver::BindingResolver;
use super::resolver_rest::RestBindingResolver;
use super::resolver_rpc::RpcBindingResolver;
use crate::idempotency::mount_prefix;

impl EnvelopeLayerBuilder {
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
        let policy: Box<dyn EnvelopePolicy> = match self.unresolved_mode {
            Some(unresolved) => Box::new(WithUnresolved { policy, unresolved }),
            None => policy,
        };
        // `mount_prefix(..)` wins over `rest(..)`/`rpc(..)`'s, in any order.
        let prefix = self
            .mount_prefix
            .as_deref()
            .or(self.transport_prefix.as_deref())
            .unwrap_or("");
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
