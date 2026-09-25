//! Why [`super::StreamableHttpServer::build`] refused a configuration.
//!
//! Every variant is a configuration an operator wrote and would otherwise
//! discover only as a silent hole in production: an Origin check that never
//! runs, or a metadata document naming a resource no client can match.
//! Refusing at startup is the point.

use std::fmt;

use crate::listing::ToolTableError;

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum HttpConfigError {
    /// The allowed-origins list was empty. `rmcp` reads an empty list as
    /// "do not check `Origin`", so an empty list is refused rather than
    /// served (ADR 0002 § Transports).
    NoAllowedOrigins,
    /// An allowed origin is not `scheme://host[:port]` or `null`.
    InvalidOrigin(String),
    /// The protected resource identifier is not an absolute `http`/`https`
    /// URL without user info, query or fragment (RFC 9728 §1.2), or its
    /// path cannot be routed.
    InvalidResource(String),
    /// MCP's authorization spec requires at least one authorization server
    /// in the metadata document.
    NoAuthorizationServers,
    /// An authorization server is not an absolute `http`/`https` URL.
    InvalidAuthorizationServer(String),
    /// A scope is not an RFC 6749 §3.3 `scope-token` (no spaces, quotes or
    /// backslashes), so it cannot be listed in the metadata or quoted in a
    /// `WWW-Authenticate` challenge.
    InvalidScope(String),
    /// An allowed host is empty.
    InvalidHost(String),
    /// The generated tool table is not servable (see [`ToolTableError`]).
    Tools(ToolTableError),
}

impl fmt::Display for HttpConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoAllowedOrigins => formatter.write_str(
                "mcp http: the allowed-origins list is empty; an empty list would disable the \
                 Origin check",
            ),
            Self::InvalidOrigin(origin) => write!(
                formatter,
                "mcp http: allowed origin `{origin}` is not `scheme://host[:port]` or `null`"
            ),
            Self::InvalidResource(resource) => write!(
                formatter,
                "mcp http: resource `{resource}` is not an absolute http(s) URL without user \
                 info, query or fragment, with a routable path"
            ),
            Self::NoAuthorizationServers => {
                formatter.write_str("mcp http: at least one authorization server is required")
            }
            Self::InvalidAuthorizationServer(server) => write!(
                formatter,
                "mcp http: authorization server `{server}` is not an absolute http(s) URL"
            ),
            Self::InvalidScope(scope) => {
                write!(
                    formatter,
                    "mcp http: scope `{scope}` is not a valid scope token"
                )
            }
            Self::InvalidHost(host) => {
                write!(formatter, "mcp http: allowed host `{host}` is empty")
            }
            Self::Tools(error) => write!(formatter, "mcp http: {error}"),
        }
    }
}

impl std::error::Error for HttpConfigError {}

impl From<ToolTableError> for HttpConfigError {
    fn from(error: ToolTableError) -> Self {
        Self::Tools(error)
    }
}
