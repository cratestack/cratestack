//! Request-scoped context: authenticated identity, structured
//! principal, transport extensions, plus the [`AuthProvider`] trait
//! that auth middlewares implement.

mod identity;
mod principal;
mod system;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::CratestackError;
use crate::value::Value;

pub use identity::CratestackAuthIdentity;
pub use principal::{PrincipalContext, PrincipalFacet};
pub use system::SystemContext;

use principal::lookup_value_path_in_map;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CratestackContext {
    pub auth: Option<CratestackAuthIdentity>,
    pub principal: Option<PrincipalContext>,
    pub extensions: BTreeMap<String, Value>,
    /// Backing flag for the `auth().isSystem()` policy builtin (issue
    /// #486 / ADR 0038 blocker B1).
    ///
    /// Deliberately **private** and `#[serde(skip)]` — this is the
    /// entire forgery boundary the feature depends on, so both
    /// properties matter independently:
    ///
    /// - Private, so no crate outside this module can flip it on a
    ///   context it already holds. The only public way to obtain a
    ///   `CratestackContext` with this set is [`SystemContext`], which has no
    ///   `From`/`TryFrom<CratestackContext>` and no constructor that accepts
    ///   an existing (e.g. request-derived) `CratestackContext` — see that
    ///   type's docs for why that upgrade path cannot exist even by
    ///   accident. Every `AuthProvider` impl in every consuming crate
    ///   can only ever produce a `CratestackContext` through the public
    ///   constructors below, all of which set this to `false`.
    /// - `#[serde(skip)]`, so the flag cannot cross a wire. On
    ///   deserialization serde leaves it at `bool::default()` (`false`)
    ///   regardless of what a payload contains — including a payload
    ///   that explicitly sets `"system": true`, which is the exact
    ///   forgery this guards against. See
    ///   `context::system::tests::forged_system_field_in_payload_is_ignored`.
    #[serde(skip)]
    system: bool,
}

/// Everything an [`AuthProvider`] gets to see about an inbound request.
///
/// `extensions` is the request's `http::Extensions` typemap as populated
/// by whatever tower/axum layers ran before authentication —
/// `ConnectInfo`, an mTLS peer identity, a tenant resolved upstream, a
/// trace/session handle, etc. Before this field existed, the only way to
/// pass such data to an `AuthProvider` was to smuggle it back through a
/// header — exactly the spoofable channel the trusted-proxy work
/// (#415/#416/#526) constrains. `extensions` is a legitimate trust
/// source distinct from `headers`/`body`: those are wire-controlled and
/// attacker-influenced, while extensions are populated in-process by
/// layers the deployer chose to install, so an `AuthProvider` can trust
/// a value found here in a way it must NOT trust the equivalent claimed
/// via a header.
///
/// A shared reference is `Copy`, so adding this field does not affect the
/// `Copy` derive below.
#[derive(Debug, Clone, Copy)]
pub struct RequestContext<'a> {
    pub method: &'a str,
    pub path: &'a str,
    pub query: Option<&'a str>,
    pub headers: &'a http::HeaderMap,
    pub body: &'a [u8],
    pub extensions: &'a http::Extensions,
}

pub trait AuthProvider: Clone + Send + Sync + 'static {
    type Error: Into<CratestackError> + Send;

    fn authenticate(
        &self,
        request: &RequestContext<'_>,
    ) -> impl ::core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send;
}

impl<F, E> AuthProvider for F
where
    F: Clone
        + Send
        + Sync
        + 'static
        + for<'a> Fn(&'a http::HeaderMap) -> Result<CratestackContext, E>,
    E: Into<CratestackError> + Send,
{
    type Error = E;

    fn authenticate(
        &self,
        request: &RequestContext<'_>,
    ) -> impl ::core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        let result = (self)(request.headers);
        ::core::future::ready(result)
    }
}

impl CratestackContext {
    pub fn anonymous() -> Self {
        Self::default()
    }

    pub fn authenticated(fields: impl IntoIterator<Item = (String, Value)>) -> Self {
        let fields = fields.into_iter().collect::<BTreeMap<_, _>>();
        Self {
            auth: Some(CratestackAuthIdentity {
                fields: fields.clone(),
            }),
            principal: Some(PrincipalContext::from_claims(fields)),
            extensions: BTreeMap::new(),
            system: false,
        }
    }

    pub fn is_authenticated(&self) -> bool {
        self.auth.is_some() || self.principal.is_some()
    }

    /// Backs the `auth().isSystem()` policy builtin. `true` only for a
    /// context minted through [`SystemContext`]; never `true` for
    /// anything an [`AuthProvider`] produced from a request, and never
    /// `true` after a deserialization round-trip. See the field doc on
    /// `CratestackContext::system` for the structural argument, not just the
    /// convention, behind that guarantee.
    pub fn is_system(&self) -> bool {
        self.system
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

    pub fn from_principal<P: Serialize>(principal: Option<P>) -> Result<Self, CratestackError> {
        let Some(principal) = principal else {
            return Ok(Self::anonymous());
        };

        let principal = PrincipalContext::from_principal(principal)?;
        let auth = principal.as_auth_identity();
        Ok(Self {
            auth: Some(auth),
            principal: Some(principal),
            extensions: BTreeMap::new(),
            system: false,
        })
    }

    pub fn with_principal(principal: PrincipalContext) -> Self {
        Self {
            auth: Some(principal.as_auth_identity()),
            principal: Some(principal),
            extensions: BTreeMap::new(),
            system: false,
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
