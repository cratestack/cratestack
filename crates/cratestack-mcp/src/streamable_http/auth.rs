//! One request through the application's [`AuthProvider`], the extension
//! point its REST and RPC routers use (ADR 0002 § Authentication: no second
//! auth mechanism).
//!
//! **A bearer token is required before the provider runs.** MCP's
//! authorization spec puts the access token in `Authorization: Bearer`, and
//! a request without one gets the 401 challenge that tells the client where
//! to authenticate. A provider that authenticates some other way (mTLS in
//! the extensions, a signed-request header) still gets called, but only for
//! requests that also carry a token.
//!
//! **An identity is required after it.** REST providers often return
//! `CratestackContext::anonymous()` for a token they do not recognise and
//! leave the rest to `@allow`. An MCP resource server must refuse that
//! request, so an unauthenticated context is a 401 here.
//!
//! **The token is never logged.** Nothing here logs a header. A provider's
//! error detail is logged, since it is how an operator learns why tokens
//! fail, but with the token cut out first, in case the provider quoted it.

use cratestack_core::{AuthProvider, CratestackContext, CratestackError, RequestContext};
use http::header::AUTHORIZATION;
use http::request::Parts;
use http::{HeaderMap, StatusCode};

use super::reply::{self, Reply};
use super::resource::Resolved;

/// The token of the request's single `Authorization: Bearer` header. Two
/// `Authorization` headers are ambiguous, so they count as none.
pub(crate) fn bearer(headers: &HeaderMap) -> Option<&str> {
    let mut values = headers.get_all(AUTHORIZATION).iter();
    let value = values.next()?;
    if values.next().is_some() {
        return None;
    }
    let (scheme, token) = value.to_str().ok()?.split_once(' ')?;
    let token = token.trim();
    (scheme.eq_ignore_ascii_case("bearer") && !token.is_empty()).then_some(token)
}

/// No token at all: RFC 6750 §3.1 says the challenge then carries no
/// `error`, only where to get one.
pub(crate) fn missing_token(resource: &Resolved) -> Reply {
    refused("no bearer token", "UNAUTHORIZED", "");
    reply::challenge(StatusCode::UNAUTHORIZED, resource.challenge(None))
}

pub(crate) async fn authenticate<A: AuthProvider>(
    provider: &A,
    resource: &Resolved,
    parts: &Parts,
    body: &[u8],
    token: &str,
) -> Result<CratestackContext, Box<Reply>> {
    let request = RequestContext {
        method: parts.method.as_str(),
        path: &resource.path,
        query: parts.uri.query(),
        headers: &parts.headers,
        body,
        extensions: &parts.extensions,
    };
    let invalid = || {
        reply::challenge(
            StatusCode::UNAUTHORIZED,
            resource.challenge(Some("invalid_token")),
        )
    };
    let error: CratestackError = match provider.authenticate(&request).await {
        Ok(context) if context.is_authenticated() => return Ok(context),
        Ok(_) => {
            refused("the provider returned no identity", "UNAUTHORIZED", "");
            return Err(Box::new(invalid()));
        }
        Err(error) => error.into(),
    };
    let detail = error.detail().unwrap_or("").replace(token, "[redacted]");
    refused("the provider refused the token", error.code(), &detail);
    let status = error.status_code();
    Err(Box::new(if status.is_server_error() {
        // The provider could not decide (a JWKS endpoint down, say). A 401
        // would send the client to re-authenticate for nothing.
        reply::from_error(error)
    } else if status == StatusCode::FORBIDDEN {
        reply::challenge(
            StatusCode::FORBIDDEN,
            resource.challenge(Some("insufficient_scope")),
        )
    } else {
        invalid()
    }))
}

fn refused(reason: &str, code: &str, detail: &str) {
    tracing::warn!(
        target: "cratestack",
        cratestack_operation = "mcp_http_auth",
        cratestack_error = code,
        cratestack_detail = detail,
        "mcp: request refused: {reason}",
    );
}
