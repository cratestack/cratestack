//! Request-scoped context: authenticated identity, structured
//! principal, transport extensions, plus the [`AuthProvider`] trait
//! that auth middlewares implement.

mod identity;
mod principal;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::CoolError;
use crate::value::Value;

pub use identity::CoolAuthIdentity;
pub use principal::{PrincipalContext, PrincipalFacet};

use principal::lookup_value_path_in_map;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CoolContext {
    pub auth: Option<CoolAuthIdentity>,
    pub principal: Option<PrincipalContext>,
    pub extensions: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Copy)]
pub struct RequestContext<'a> {
    pub method: &'a str,
    pub path: &'a str,
    pub query: Option<&'a str>,
    pub headers: &'a http::HeaderMap,
    pub body: &'a [u8],
}

pub trait AuthProvider: Clone + Send + Sync + 'static {
    type Error: Into<CoolError> + Send;

    fn authenticate(
        &self,
        request: &RequestContext<'_>,
    ) -> impl ::core::future::Future<Output = Result<CoolContext, Self::Error>> + Send;
}

impl<F, E> AuthProvider for F
where
    F: Clone + Send + Sync + 'static + for<'a> Fn(&'a http::HeaderMap) -> Result<CoolContext, E>,
    E: Into<CoolError> + Send,
{
    type Error = E;

    fn authenticate(
        &self,
        request: &RequestContext<'_>,
    ) -> impl ::core::future::Future<Output = Result<CoolContext, Self::Error>> + Send {
        let result = (self)(request.headers);
        ::core::future::ready(result)
    }
}

impl CoolContext {
    pub fn anonymous() -> Self {
        Self::default()
    }

    pub fn authenticated(fields: impl IntoIterator<Item = (String, Value)>) -> Self {
        let fields = fields.into_iter().collect::<BTreeMap<_, _>>();
        Self {
            auth: Some(CoolAuthIdentity {
                fields: fields.clone(),
            }),
            principal: Some(PrincipalContext::from_claims(fields)),
            extensions: BTreeMap::new(),
        }
    }

    pub fn is_authenticated(&self) -> bool {
        self.auth.is_some() || self.principal.is_some()
    }

    pub fn auth_field(&self, name: &str) -> Option<&Value> {
        if let Some(auth) = self.auth.as_ref()
            && let Some(value) = auth
                .fields
                .get(name)
                .or_else(|| lookup_value_path_in_map(&auth.fields, name))
        {
            return Some(value);
        }

        self.principal
            .as_ref()
            .and_then(|principal| principal.field(name))
    }

    pub fn from_principal<P: Serialize>(principal: Option<P>) -> Result<Self, CoolError> {
        let Some(principal) = principal else {
            return Ok(Self::anonymous());
        };

        let principal = PrincipalContext::from_principal(principal)?;
        let auth = principal.as_auth_identity();
        Ok(Self {
            auth: Some(auth),
            principal: Some(principal),
            extensions: BTreeMap::new(),
        })
    }

    pub fn with_principal(principal: PrincipalContext) -> Self {
        Self {
            auth: Some(principal.as_auth_identity()),
            principal: Some(principal),
            extensions: BTreeMap::new(),
        }
    }

    /// Convenience accessor for the principal's actor id. Falls back
    /// from `principal.actor.id` to `principal.claims.id` to
    /// `auth.fields.id` so audit rows capture an identity regardless
    /// of which builder the caller used.
    pub fn principal_actor_id(&self) -> Option<&str> {
        let from_facet = self
            .principal
            .as_ref()
            .and_then(|p| p.actor.as_ref())
            .and_then(|facet| facet.fields.get("id"));
        let from_claims = self.principal.as_ref().and_then(|p| p.claims.get("id"));
        let from_auth = self.auth.as_ref().and_then(|auth| auth.fields.get("id"));
        from_facet
            .or(from_claims)
            .or(from_auth)
            .and_then(|v| match v {
                Value::String(s) => Some(s.as_str()),
                _ => None,
            })
    }

    /// Tenant id surfaced for audit/log scoping.
    pub fn tenant_id(&self) -> Option<&str> {
        self.principal
            .as_ref()
            .and_then(|p| p.tenant.as_ref())
            .and_then(|facet| facet.fields.get("id"))
            .and_then(|v| match v {
                Value::String(s) => Some(s.as_str()),
                _ => None,
            })
    }

    /// Client IP, if the auth provider injected one (e.g. from
    /// `X-Forwarded-For` or the socket remote-addr).
    pub fn client_ip(&self) -> Option<&str> {
        self.extensions.get("client_ip").and_then(|v| match v {
            Value::String(s) => Some(s.as_str()),
            _ => None,
        })
    }

    /// W3C `traceparent` value, if surfaced into the context by the
    /// correlation-id middleware.
    pub fn request_id(&self) -> Option<&str> {
        self.extensions.get("request_id").and_then(|v| match v {
            Value::String(s) => Some(s.as_str()),
            _ => None,
        })
    }

    /// Snapshot of principal claims for audit recording — full map
    /// regardless of nesting depth. Empty for anonymous contexts.
    pub fn audit_claims_snapshot(&self) -> BTreeMap<String, Value> {
        self.principal
            .as_ref()
            .map(|p| p.claims.clone())
            .unwrap_or_default()
    }

    /// Attach a W3C `traceparent`-style request id to the context.
    /// Surfaces in tracing spans and is recorded on audit events so
    /// SIEM tools can stitch the trail across systems.
    pub fn with_request_id(mut self, request_id: impl Into<String>) -> Self {
        self.extensions
            .insert("request_id".to_owned(), Value::String(request_id.into()));
        self
    }

    /// Attach a client IP for the same reasons as
    /// [`Self::with_request_id`]. Banks generally derive this from
    /// `X-Forwarded-For` or the socket address inside the auth
    /// provider.
    pub fn with_client_ip(mut self, ip: impl Into<String>) -> Self {
        self.extensions
            .insert("client_ip".to_owned(), Value::String(ip.into()));
        self
    }
}
