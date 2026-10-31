//! The guard's credential check, before the body is read: an
//! `Authorization` it will not guess at is a 400 under every rule, and a
//! request the [`CredentialPresence`](super::CredentialPresence) rule finds
//! without a credential gets the 401 challenge.

use cratestack_core::{AuthProvider, RequestContext};
use http::StatusCode;
use http::request::Parts;

use super::auth::{Bearer, Credential, bearer, missing_token};
use super::guard::Shared;
use super::reply::{self, Reply};
use crate::table::McpTools;

impl<T: McpTools, A: AuthProvider> Shared<T, A> {
    /// The request's credential under the configured rule, or the 400 or
    /// 401 that refuses it before the body is read.
    pub(crate) fn credential(&self, parts: &Parts) -> Result<Credential, Box<Reply>> {
        let token = match bearer(&parts.headers) {
            Bearer::Token(token) => Some(token),
            Bearer::Missing => None,
            Bearer::Malformed => return Err(self.malformed()),
        };
        let mut secrets = self.presence.secrets(parts, token);
        if self.presence.asks_provider() {
            match self.provider.credential(&self.head(parts)) {
                cratestack_core::Credential::Absent => {
                    return Err(Box::new(missing_token(&self.resource, "no credential")));
                }
                cratestack_core::Credential::Malformed(_) => return Err(self.malformed()),
                cratestack_core::Credential::Present {
                    redact: Some(redact),
                } if !redact.trim().is_empty() => secrets.push(redact.trim().to_owned()),
                _ => {}
            }
        } else if !self.presence.present(parts, token.is_some()) {
            let reason = if self.presence.is_bearer() {
                "no bearer token"
            } else {
                "no credential"
            };
            return Err(Box::new(missing_token(&self.resource, reason)));
        }
        Ok(Credential {
            bearer: token.is_some(),
            secrets,
        })
    }

    /// The request head, as the provider's `credential` reads it: the
    /// canonical path of the resource and no body (it is not read yet).
    fn head<'r>(&'r self, parts: &'r Parts) -> RequestContext<'r> {
        RequestContext {
            method: parts.method.as_str(),
            path: &self.resource.path,
            query: parts.uri.query(),
            headers: &parts.headers,
            body: &[],
            extensions: &parts.extensions,
        }
    }

    fn malformed(&self) -> Box<Reply> {
        let header = self.resource.challenge(Some("invalid_request"));
        Box::new(reply::challenge(StatusCode::BAD_REQUEST, header))
    }
}
