//! [`Credential`]: what an [`AuthProvider`](crate::AuthProvider) says
//! about a request before authenticating it — whether the request carries
//! the provider's kind of credential at all (ADR 0021 §4, cratestack#1243).
//!
//! Two callers ask:
//!
//! - the [first-success chain](crate::first_success), which skips a member
//!   whose credential is [`Credential::Absent`] without calling it, so a
//!   strict provider (one that refuses a request without its credential)
//!   can sit in a chain;
//! - MCP Streamable HTTP, when a deployment opts in with
//!   `CredentialPresence::ask_provider()`, to answer a request without a
//!   credential with its 401 challenge before the body is read.
//!
//! The answer is read from the request head: the method, path, query,
//! headers and extensions. **The body may be empty when the provider is
//! asked** (MCP asks before reading it), so a provider never looks for its
//! credential there.

use http::HeaderMap;
use http::header::AUTHORIZATION;

/// Whether a request carries a provider's credential. See the module
/// documentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Credential<'r> {
    /// The provider cannot tell without authenticating. The default for
    /// every provider: it is always called, exactly as before
    /// [`AuthProvider::credential`](crate::AuthProvider::credential)
    /// existed.
    Unknown,
    /// The request carries none of this provider's credentials. A chain
    /// skips the provider; MCP answers with its 401 challenge.
    Absent,
    /// The request carries one. `redact` is a value to cut out of logged
    /// error detail (a token), if the provider wants one cut.
    Present { redact: Option<&'r str> },
    /// The request carries one the provider will not read (two
    /// `Authorization` headers, a scheme with no token). MCP answers 400
    /// `invalid_request`; a chain still calls the provider, which refuses.
    Malformed(&'static str),
}

impl<'r> Credential<'r> {
    /// An `Authorization: Bearer <token>` credential, read the way MCP
    /// reads it: no header or another scheme is [`Credential::Absent`], one
    /// well-formed token is [`Credential::Present`] with the token to
    /// redact, and two headers, a value that is not text or `Bearer` with
    /// no token is [`Credential::Malformed`].
    pub fn bearer(headers: &'r HeaderMap) -> Self {
        let mut values = headers.get_all(AUTHORIZATION).iter();
        let Some(value) = values.next() else {
            return Self::Absent;
        };
        if values.next().is_some() {
            return Self::Malformed("more than one Authorization header");
        }
        let Ok(value) = value.to_str() else {
            return Self::Malformed("an Authorization header that is not text");
        };
        let (scheme, token) = value.split_once(' ').unwrap_or((value, ""));
        if !scheme.eq_ignore_ascii_case("bearer") {
            return Self::Absent;
        }
        match token.trim() {
            "" => Self::Malformed("a Bearer scheme with no token"),
            token => Self::Present {
                redact: Some(token),
            },
        }
    }

    /// A credential carried in `name`: absent without the header, present
    /// (with nothing to redact) with it. For a provider whose credential is
    /// a header of its own, such as a device signature.
    pub fn header(headers: &'r HeaderMap, name: impl http::header::AsHeaderName) -> Self {
        if headers.contains_key(name) {
            Self::Present { redact: None }
        } else {
            Self::Absent
        }
    }

    /// Whether this is [`Credential::Absent`].
    pub const fn is_absent(&self) -> bool {
        matches!(self, Self::Absent)
    }
}

#[cfg(test)]
#[path = "credential_tests.rs"]
mod tests;
