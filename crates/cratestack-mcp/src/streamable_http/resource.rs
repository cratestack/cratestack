//! The OAuth 2.1 protected-resource side of Streamable HTTP (MCP 2026-07-28
//! authorization; RFC 9728 Protected Resource Metadata). `rmcp` has no
//! server-side counterpart, so this crate builds it (ADR 0002 § Authentication).
//!
//! **Everything is derived from the resource identifier, never from the
//! request path.** Under `Router::nest` the service sees a path with the
//! mount prefix stripped, so a challenge or metadata URL built from the
//! request would name the wrong resource. The identifier is the public URL
//! of the endpoint, fixed at build time, so nesting cannot change it. For
//! the same reason it is the `path` the `AuthProvider` sees, as REST passes
//! its declared route rather than the raw one.

use bytes::Bytes;
use http::HeaderValue;
use serde_json::json;

use super::error::HttpConfigError;

/// RFC 9728 §3's well-known suffix.
const WELL_KNOWN: &str = "/.well-known/oauth-protected-resource";

/// What the metadata document says about this MCP endpoint.
///
/// `resource` is the endpoint's public URL, for example
/// `https://api.example.com/mcp`. It is what an audience-checking
/// `AuthProvider` should compare a token's `aud` to. `authorization_servers`
/// are the issuers a client may get a token from; MCP requires at least one.
#[derive(Debug, Clone)]
pub struct ProtectedResource {
    resource: String,
    authorization_servers: Vec<String>,
    scopes: Vec<String>,
}

impl ProtectedResource {
    pub fn new(
        resource: impl Into<String>,
        authorization_servers: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            resource: resource.into(),
            authorization_servers: authorization_servers.into_iter().map(Into::into).collect(),
            scopes: Vec::new(),
        }
    }

    /// Scopes listed as `scopes_supported` and named in every 401
    /// challenge's `scope`, so a client knows what to ask for.
    pub fn with_scopes(mut self, scopes: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.scopes = scopes.into_iter().map(Into::into).collect();
        self
    }
}

/// A validated [`ProtectedResource`], with everything the guard and the
/// metadata router need precomputed.
#[derive(Debug, Clone)]
pub(crate) struct Resolved {
    pub(crate) document: Bytes,
    pub(crate) metadata_url: String,
    /// Path-suffixed form first, then the root form (MCP clients try both).
    pub(crate) well_known_paths: Vec<String>,
    /// `host[:port]` of the identifier, the default allowed `Host`.
    pub(crate) authority: String,
    /// The identifier's path, what the `AuthProvider` is told was called.
    pub(crate) path: String,
    challenge_params: String,
}

impl Resolved {
    /// `WWW-Authenticate` for a 401 (`error` absent when no token was sent,
    /// RFC 6750 §3.1) or a 403 (`insufficient_scope`).
    pub(crate) fn challenge(&self, error: Option<&str>) -> HeaderValue {
        let value = match error {
            Some(error) => format!("Bearer error=\"{error}\", {}", self.challenge_params),
            None => format!("Bearer {}", self.challenge_params),
        };
        // Every part was validated to be visible ASCII without quotes.
        HeaderValue::from_str(&value).unwrap_or_else(|_| HeaderValue::from_static("Bearer"))
    }
}

fn absolute(url: &str) -> Option<http::Uri> {
    let uri = http::Uri::try_from(url).ok()?;
    let authority = uri.authority()?;
    let ok = matches!(uri.scheme_str(), Some("http" | "https"))
        && !authority.host().is_empty()
        && !authority.as_str().contains('@')
        && !url.contains('#');
    ok.then_some(uri)
}

/// RFC 6749 §3.3: `scope-token = 1*( %x21 / %x23-5B / %x5D-7E )`.
fn scope_token(scope: &str) -> bool {
    !scope.is_empty()
        && scope
            .bytes()
            .all(|b| b == 0x21 || (0x23..=0x5B).contains(&b) || (0x5D..=0x7E).contains(&b))
}

impl ProtectedResource {
    pub(crate) fn resolve(self) -> Result<Resolved, HttpConfigError> {
        let invalid = || HttpConfigError::InvalidResource(self.resource.clone());
        let uri = absolute(&self.resource).ok_or_else(invalid)?;
        // A query is refused (RFC 9728 §1.2 SHOULD NOT), and so is a path
        // `axum` would read as a capture, since it becomes a route.
        let path = uri.path();
        if uri.query().is_some() || path.contains(['{', '}', '*', ':']) {
            return Err(invalid());
        }
        if self.authorization_servers.is_empty() {
            return Err(HttpConfigError::NoAuthorizationServers);
        }
        for server in &self.authorization_servers {
            if absolute(server).is_none() {
                return Err(HttpConfigError::InvalidAuthorizationServer(server.clone()));
            }
        }
        if let Some(bad) = self.scopes.iter().find(|scope| !scope_token(scope)) {
            return Err(HttpConfigError::InvalidScope(bad.clone()));
        }

        // RFC 9728 §3.1: the well-known suffix goes between the host and the
        // path, so `https://h/api/mcp` is described at
        // `https://h/.well-known/oauth-protected-resource/api/mcp`.
        let suffix = if path == "/" { "" } else { path };
        let suffixed = format!("{WELL_KNOWN}{suffix}");
        let authority = uri.authority().map(|a| a.as_str()).unwrap_or_default();
        let metadata_url = format!(
            "{}://{authority}{suffixed}",
            uri.scheme_str().unwrap_or("https")
        );
        let mut well_known_paths = vec![suffixed];
        if !suffix.is_empty() {
            well_known_paths.push(WELL_KNOWN.to_owned());
        }

        let mut challenge_params = format!("resource_metadata=\"{metadata_url}\"");
        let mut document = json!({
            "resource": self.resource,
            "authorization_servers": self.authorization_servers,
            "bearer_methods_supported": ["header"],
        });
        if !self.scopes.is_empty() {
            challenge_params.push_str(&format!(", scope=\"{}\"", self.scopes.join(" ")));
            document["scopes_supported"] = json!(self.scopes);
        }
        Ok(Resolved {
            document: Bytes::from(document.to_string()),
            metadata_url,
            well_known_paths,
            authority: authority.to_owned(),
            path: path.to_owned(),
            challenge_params,
        })
    }
}
