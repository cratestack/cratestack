//! The HTTP side's `AuthProvider` (cratestack#1041, #1245).
//!
//! Two credentials carry the same example token (`src/token.rs`):
//!
//! - `Authorization: Bearer <token>`, what an MCP client sends after the
//!   RFC 9728 discovery the 401 challenge points it to;
//! - `X-Agent-Token: <token>`, a credential that is **not** a bearer token,
//!   as an agent runtime with its own header might send it.
//!
//! The endpoint runs with `CredentialPresence::ask_provider()` (ADR 0021
//! §4), so the transport asks [`AuthProvider::credential`] whether a
//! request carries either one, instead of insisting on `Bearer`. A request
//! with neither still gets MCP's own 401 and its metadata pointer.

use cratestack::{AuthProvider, CratestackContext, CratestackError, Credential, RequestContext};

use crate::token::{TokenVerifier, refuse};

/// The non-bearer header.
pub const AGENT_TOKEN: &str = "x-agent-token";

/// Checks the token in either credential with a [`TokenVerifier`] whose
/// audience is the MCP endpoint's URL.
#[derive(Clone)]
pub struct AudienceProvider {
    verifier: TokenVerifier,
}

impl AudienceProvider {
    pub fn new(verifier: TokenVerifier) -> Self {
        Self { verifier }
    }
}

fn agent_token<'r>(request: &RequestContext<'r>) -> Option<&'r str> {
    request.headers.get(AGENT_TOKEN)?.to_str().ok()
}

impl AuthProvider for AudienceProvider {
    type Error = CratestackError;

    async fn authenticate(
        &self,
        request: &RequestContext<'_>,
    ) -> Result<CratestackContext, CratestackError> {
        if let Some(token) = agent_token(request) {
            return self.verifier.verify(token);
        }
        let header = request
            .headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| refuse("no token"))?;
        let token = header
            .strip_prefix("Bearer ")
            .ok_or_else(|| refuse("not a bearer token"))?;
        self.verifier.verify(token)
    }

    /// Present when either credential is; the agent token is named for
    /// redaction, as the transport already redacts a bearer token.
    fn credential<'r>(&self, request: &RequestContext<'r>) -> Credential<'r> {
        if let Some(token) = agent_token(request) {
            return Credential::Present {
                redact: Some(token),
            };
        }
        Credential::bearer(request.headers)
    }
}
