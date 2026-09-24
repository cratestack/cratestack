//! The stdio transport (ADR 0002 § Transports, Q1).
//!
//! Newline-delimited JSON-RPC on stdin/stdout, via `rmcp`'s `transport-io`.
//! Three properties, each pinned by `tests/stdio_process.rs`, which runs a
//! real child process:
//!
//! - **stdout carries only MCP messages.** Nothing in this crate prints, and
//!   neither does `rmcp`; both log through `tracing`. Where `tracing` goes is
//!   the application's subscriber, so point it at stderr (example below).
//!   A subscriber writing to stdout would corrupt the protocol stream.
//! - **The server exits when stdin closes.** [`StdioServer::serve`] returns
//!   `Ok(())` on end of input, including input that ends before any request.
//! - **The caller's identity is an argument.** [`StdioServer::new`] takes the
//!   [`CratestackContext`] every call runs under. There is no default and no
//!   "local means trusted" path: the spec says stdio credentials come from
//!   the environment, and turning them into a context is the application's
//!   job, done deliberately.
//!
//! ```no_run
//! use cratestack_core::{CratestackContext, SystemContext, Value};
//! use cratestack_mcp::{McpTools, StdioServer};
//!
//! // `tools` is the generated table, e.g. `cratestack_schema::mcp::tools(db,
//! // registry, resolvers)`; the facade re-exports these types as
//! // `cratestack::{CratestackContext, SystemContext}` and
//! // `cratestack::mcp::StdioServer`.
//!
//! // A deliberate service identity, for a server an operator runs on the
//! // service's own behalf. Policies see `auth().isSystem()`.
//! async fn as_service(tools: impl McpTools) -> Result<(), Box<dyn std::error::Error>> {
//!     tracing_subscriber::fmt().with_writer(std::io::stderr).init();
//!     let ctx = SystemContext::for_service("support-agent").into_context();
//!     StdioServer::new(tools, ctx)?.serve().await?;
//!     Ok(())
//! }
//!
//! // A user's identity, from a token in the environment that the
//! // application verified with its own verifier.
//! async fn as_user(
//!     tools: impl McpTools,
//!     verify: impl Fn(&str) -> Result<Vec<(String, Value)>, Box<dyn std::error::Error>>,
//! ) -> Result<(), Box<dyn std::error::Error>> {
//!     let token = std::env::var("SUPPORT_AGENT_TOKEN")?;
//!     let claims = verify(&token)?;
//!     let ctx = CratestackContext::authenticated(claims);
//!     StdioServer::new(tools, ctx)?.serve().await?;
//!     Ok(())
//! }
//! ```

use std::fmt;

use cratestack_core::CratestackContext;
use cratestack_exec::{OpExecutor, StoreErrorPolicy};
use rmcp::ServiceExt;
use rmcp::service::{QuitReason, ServerInitializeError};
use tokio::io::{AsyncRead, AsyncWrite};

use crate::listing::ToolTableError;
use crate::server::McpServer;
use crate::table::McpTools;

/// Builds and runs an [`McpServer`] over stdio.
pub struct StdioServer<T: McpTools> {
    server: McpServer<T>,
}

impl<T: McpTools> StdioServer<T> {
    /// `context` is required; see the module doc for why nothing defaults it.
    pub fn new(tools: T, context: CratestackContext) -> Result<Self, ToolTableError> {
        Ok(Self {
            server: McpServer::new(tools, context)?,
        })
    }

    /// Opt in to L3 rate-limit and idempotency admission. See
    /// [`McpServer::with_executor`].
    pub fn with_executor(mut self, executor: OpExecutor) -> Self {
        self.server = self.server.with_executor(executor);
        self
    }

    /// What a failing rate-limit store does to a call. See
    /// [`McpServer::with_store_error_policy`].
    pub fn with_store_error_policy(mut self, policy: StoreErrorPolicy) -> Self {
        self.server = self.server.with_store_error_policy(policy);
        self
    }

    /// Serve the process's stdin and stdout until stdin closes.
    pub async fn serve(self) -> Result<(), ServeError> {
        self.serve_io(tokio::io::stdin(), tokio::io::stdout()).await
    }

    /// Serve any byte stream pair, with stdio's framing. What
    /// [`Self::serve`] runs on the real stdin/stdout; tests run it on an
    /// in-memory duplex.
    pub async fn serve_io<R, W>(self, reader: R, writer: W) -> Result<(), ServeError>
    where
        R: AsyncRead + Send + Unpin + 'static,
        W: AsyncWrite + Send + Unpin + 'static,
    {
        let running = match self.server.serve((reader, writer)).await {
            Ok(running) => running,
            // Input ended before a first request: nothing to serve.
            Err(ServerInitializeError::ConnectionClosed(_)) => return Ok(()),
            Err(error) => return Err(ServeError(error.to_string())),
        };
        match running.waiting().await {
            Ok(QuitReason::Closed | QuitReason::Cancelled) => Ok(()),
            Ok(QuitReason::JoinError(error)) => Err(ServeError(error.to_string())),
            // `QuitReason` is `#[non_exhaustive]`; a reason this build does
            // not know is reported, not mistaken for a clean end of input.
            Ok(_) => Err(ServeError("an unrecognised quit reason".to_owned())),
            Err(error) => Err(ServeError(error.to_string())),
        }
    }
}

/// The server stopped for a reason other than its input ending.
///
/// Under 2026-07-28 the first message is an ordinary request, and `rmcp`
/// ends the connection when that request is malformed (for instance a
/// legacy `initialize` for a revision this server does not speak) after
/// answering it with a JSON-RPC error. That lands here.
#[derive(Debug)]
pub struct ServeError(String);

impl fmt::Display for ServeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "mcp stdio server stopped: {}", self.0)
    }
}

impl std::error::Error for ServeError {}
