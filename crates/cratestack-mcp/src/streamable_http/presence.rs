//! [`CredentialPresence`]: whether a request carries a credential at all,
//! decided before the body is read and before the provider runs
//! (cratestack#1243).
//!
//! The default is MCP's own: an `Authorization: Bearer` token, exactly as
//! before this type existed. A provider that authenticates another way (a
//! device signature over the body, a COSE envelope, mTLS in the request's
//! extensions) could never be reached under that rule, so an application
//! can let the provider decide, or name its own test.
//!
//! Whatever the rule, two things hold:
//!
//! - **The challenge is kept.** A request the rule finds without a
//!   credential gets the same 401 `Bearer resource_metadata="..."`
//!   challenge (RFC 9728) a request without a token gets today.
//! - **The credential is not forwarded.** `Authorization` is always
//!   removed before `rmcp` sees the request, and so is every header named
//!   with [`CredentialPresence::with_credential_headers`]. Their values are
//!   also cut out of a provider's logged error detail, as a bearer token is.

use std::fmt;
use std::sync::Arc;

use http::HeaderName;
use http::request::Parts;

type Predicate = Arc<dyn Fn(&Parts) -> bool + Send + Sync>;

#[derive(Clone)]
enum Rule {
    Bearer,
    Provider,
    Ask,
    Predicate(Predicate),
}

/// How the Streamable HTTP guard decides that a request carries a
/// credential, set with
/// [`StreamableHttpServer::with_credential_presence`](super::StreamableHttpServer::with_credential_presence).
/// See the module documentation of `cratestack_mcp::streamable_http`.
#[derive(Clone)]
pub struct CredentialPresence {
    rule: Rule,
    headers: Vec<HeaderName>,
}

impl CredentialPresence {
    /// The default: a request needs `Authorization: Bearer <token>`, and
    /// one without gets the 401 challenge before the body is read.
    pub fn bearer() -> Self {
        Self::with_rule(Rule::Bearer)
    }

    /// Every request reaches the provider, which alone decides. A request
    /// without a credential then costs a body read and a provider call
    /// before its 401. A provider that returns no identity for a request
    /// without a bearer token gets the challenge without an `error`, as a
    /// request without a token does under [`Self::bearer`].
    pub fn provider_decides() -> Self {
        Self::with_rule(Rule::Provider)
    }

    /// The provider's own [`AuthProvider::credential`] answers, before the
    /// body is read (with an empty body): `Absent` gets the 401 challenge,
    /// `Malformed` a 400 `invalid_request`, `Present` reaches the provider
    /// with its `redact` value cut out of logged detail, and `Unknown` (the
    /// default of a provider that does not say) reaches the provider as
    /// under [`Self::provider_decides`] (ADR 0021 §4).
    ///
    /// [`AuthProvider::credential`]: cratestack_core::AuthProvider::credential
    pub fn ask_provider() -> Self {
        Self::with_rule(Rule::Ask)
    }

    /// A request reaches the provider when `present` says it carries a
    /// credential (it sees the method, URI, headers and extensions, never
    /// the body); otherwise it gets the 401 challenge before the body is
    /// read. `present` is the whole rule: to accept a bearer token as well,
    /// test for it too.
    pub fn when(present: impl Fn(&Parts) -> bool + Send + Sync + 'static) -> Self {
        Self::with_rule(Rule::Predicate(Arc::new(present)))
    }

    /// Headers that carry the credential besides `Authorization` (for
    /// instance `x-attestation` and `x-signature`). They are removed before
    /// `rmcp` sees the request, so no tool and nothing downstream holds
    /// them, and their values are cut out of a provider's logged detail.
    #[must_use]
    pub fn with_credential_headers(mut self, names: impl IntoIterator<Item = HeaderName>) -> Self {
        self.headers.extend(names);
        self
    }

    fn with_rule(rule: Rule) -> Self {
        Self {
            rule,
            headers: Vec::new(),
        }
    }

    /// Whether this is the default rule, for the log line of a refusal.
    pub(crate) fn is_bearer(&self) -> bool {
        matches!(self.rule, Rule::Bearer)
    }

    /// Whether the provider's own `credential` answers (see
    /// [`Self::ask_provider`]).
    pub(crate) fn asks_provider(&self) -> bool {
        matches!(self.rule, Rule::Ask)
    }

    /// Whether the request carries a credential under this rule.
    /// `has_bearer` is what the guard already read from `Authorization`.
    pub(crate) fn present(&self, parts: &Parts, has_bearer: bool) -> bool {
        match &self.rule {
            Rule::Bearer => has_bearer,
            Rule::Provider | Rule::Ask => true,
            Rule::Predicate(present) => present(parts),
        }
    }

    /// The values to cut out of a logged provider detail: the bearer token
    /// and every credential header's value. Never an empty string, which
    /// `str::replace` would match everywhere.
    pub(crate) fn secrets(&self, parts: &Parts, bearer: Option<&str>) -> Vec<String> {
        let headers = self
            .headers
            .iter()
            .flat_map(|name| parts.headers.get_all(name))
            .filter_map(|value| value.to_str().ok());
        bearer
            .into_iter()
            .chain(headers)
            .map(str::trim)
            .filter(|secret| !secret.is_empty())
            .map(str::to_owned)
            .collect()
    }

    /// Removes the credential headers before `rmcp` sees the request
    /// (`Authorization` is removed by `caller::hand_over`).
    pub(crate) fn strip(&self, parts: &mut Parts) {
        for name in &self.headers {
            parts.headers.remove(name);
        }
    }
}

impl Default for CredentialPresence {
    fn default() -> Self {
        Self::bearer()
    }
}

impl fmt::Debug for CredentialPresence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let rule = match self.rule {
            Rule::Bearer => "bearer",
            Rule::Provider => "provider_decides",
            Rule::Ask => "ask_provider",
            Rule::Predicate(_) => "when",
        };
        f.debug_struct("CredentialPresence")
            .field("rule", &rule)
            .field("headers", &self.headers)
            .finish()
    }
}

#[cfg(test)]
#[path = "tests_presence.rs"]
mod tests;
