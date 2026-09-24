//! The Streamable HTTP transport (ADR 0002 § Transports and § Authentication,
//! cratestack#1039).
//!
//! A tower service the application mounts on its own axum router,
//! authenticated by the application's own `AuthProvider`: the extension
//! point its REST and RPC routers use. It speaks MCP `2026-07-28` only:
//! one `POST` endpoint, no sessions, every request self-contained.
//!
//! What this adds around `rmcp`'s service, and why each is here rather than
//! left to `rmcp`:
//!
//! - **Origin enforcement** (`origin.rs`). `rmcp` skips its check when the
//!   list is empty, so the list is a required argument, refused when empty,
//!   checked here first and by `rmcp` again.
//! - **The OAuth 2.1 resource-server contract** (`auth.rs`, `resource.rs`,
//!   `metadata.rs`). `rmcp` has none on the server side: a 401 with
//!   `WWW-Authenticate: Bearer resource_metadata="..."`, and RFC 9728
//!   metadata naming the authorization servers.
//! - **The caller's identity** (`caller.rs`). The provider's
//!   `CratestackContext` travels to the handler in the request's
//!   extensions, and every tool call runs under exactly that context,
//!   through the same L3 admission and generated policy check as stdio.
//!
//! **Audience.** MCP requires a resource server to refuse tokens issued for
//! another resource. That check belongs to the application's provider,
//! which knows its token format. CrateStack ships no generic OAuth provider
//! in v1 (ADR 0002 Q5). `tests/support/token.rs` has an example that checks
//! `aud`.
//!
//! ```no_run
//! use cratestack_core::{AuthProvider, CratestackContext, CratestackError};
//! use cratestack_mcp::{McpTools, ProtectedResource, StreamableHttpServer};
//!
//! fn app(
//!     tools: impl McpTools,
//!     provider: impl AuthProvider,
//! ) -> Result<axum::Router, Box<dyn std::error::Error>> {
//!     let resource = ProtectedResource::new(
//!         "https://api.example.com/api/mcp",
//!         ["https://auth.example.com"],
//!     )
//!     .with_scopes(["mcp:tools"]);
//!     let mcp = StreamableHttpServer::builder(
//!         tools,
//!         provider,
//!         ["https://app.example.com"],
//!         resource,
//!     )
//!     .build()?;
//!     // The endpoint goes where the identifier says (`/api/mcp`), nested
//!     // or not; the metadata is merged at the root, because RFC 9728 puts
//!     // it at `/.well-known/oauth-protected-resource/api/mcp`.
//!     let api = axum::Router::new().nest_service("/mcp", mcp.service());
//!     Ok(axum::Router::new()
//!         .nest("/api", api)
//!         .merge(mcp.metadata_router()))
//! }
//! ```

mod auth;
mod builder;
pub(crate) mod caller;
mod error;
mod guard;
mod metadata;
mod origin;
mod reply;
mod resource;

pub use builder::{StreamableHttp, StreamableHttpServer};
pub use error::HttpConfigError;
pub use guard::StreamableHttpService;
pub use resource::ProtectedResource;
