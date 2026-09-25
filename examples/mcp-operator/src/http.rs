//! The Streamable HTTP side: the MCP endpoint behind the example
//! [`AudienceProvider`], plus its RFC 9728 metadata document.

use cratestack::axum::Router;
use cratestack::mcp::{HttpConfigError, ProtectedResource, StreamableHttpServer};

use crate::token::{AudienceProvider, ISSUER, TokenVerifier};
use crate::{cratestack_schema, mcp_table};

/// What the HTTP server needs to know about where it is reached.
pub struct HttpConfig {
    /// The endpoint's public URL, e.g. `http://127.0.0.1:8787/mcp`. A token
    /// must name exactly this as its audience, and the metadata document
    /// names it as the resource.
    pub resource: String,
    /// Browser origins allowed to call the endpoint. May not be empty:
    /// `cratestack-mcp` refuses to build without one, since `rmcp` reads an
    /// empty list as "do not check `Origin`".
    pub allowed_origins: Vec<String>,
    /// The token signing key (see `src/token.rs`).
    pub key: Vec<u8>,
}

/// The whole HTTP application: `/mcp` and the metadata document, which
/// RFC 9728 puts at the host root whatever path the endpoint has.
pub fn app(db: cratestack_schema::Cratestack, config: &HttpConfig) -> Result<Router, String> {
    let verifier = TokenVerifier::new(&config.key, &config.resource)?;
    let server = StreamableHttpServer::builder(
        mcp_table(db),
        AudienceProvider::new(verifier),
        config.allowed_origins.iter().cloned(),
        ProtectedResource::new(config.resource.as_str(), [ISSUER]),
    )
    .with_implementation("mcp-operator-example", env!("CARGO_PKG_VERSION"))
    .build()
    .map_err(|error: HttpConfigError| error.to_string())?;
    Ok(Router::new()
        .nest_service("/mcp", server.service())
        .merge(server.metadata_router()))
}
