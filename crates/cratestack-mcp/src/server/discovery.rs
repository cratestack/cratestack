//! What the server says about itself: the one version it speaks, the
//! `serverInfo` it reports, `get_info`'s config, and the two methods whose
//! answer is that config or a constant, `server/discover` and
//! `completion/complete`. Split from `server.rs` for the file-length ceiling.
//!
//! Neither answer depends on who asks, and `rmcp`'s own defaults would
//! serve both whoever did. Both resolve the caller anyway (maintainer
//! decision on cratestack#1040), so that over Streamable HTTP no method
//! answers a request that reached the handler without the guard's caller:
//! each fails closed with `-32603`, like `tools/list` and `tools/call`.
//! Over stdio the caller is fixed and the check always passes.
//!
//! `discovered` and `completed` take the request's extensions rather than a
//! `RequestContext`, as `resources::listed` does, because `rmcp` does not
//! let a test build one. The `ServerHandler` overrides in `server.rs` are
//! their only callers; `rmcp` reaches those through its generic
//! `handle_request` dispatch, on stdio and on both Streamable HTTP paths.

use std::borrow::Cow;

use rmcp::model::{
    CompleteResult, DiscoverResult, Extensions, Implementation, ProtocolVersion,
    ResourcesCapability, ServerCapabilities, ServerConfig,
};
use rmcp::{ErrorData, ServerHandler};

use super::McpServer;
use crate::table::McpTools;

/// The single version [`ServerHandler::supported_protocol_versions`]
/// returns. A `static` because the trait wants a `'static` slice.
pub(super) static SUPPORTED: [ProtocolVersion; 1] = [ProtocolVersion::V_2026_07_28];

impl<T: McpTools> McpServer<T> {
    /// The `serverInfo` `server/discover` reports. Defaults to this crate's
    /// own name and version.
    pub fn with_implementation(mut self, name: &str, version: &str) -> Self {
        self.implementation = Implementation::new(name, version);
        self
    }

    /// `get_info`'s answer.
    pub(super) fn config(&self) -> ServerConfig {
        let mut capabilities = ServerCapabilities::builder().enable_tools().build();
        // Only a table with resources advertises them (cratestack#1040).
        if !self.tools.resources().is_empty() {
            capabilities.resources = Some(ResourcesCapability::default());
        }
        let mut config = ServerConfig::new(capabilities);
        config.protocol_version = ProtocolVersion::V_2026_07_28;
        config.server_info = self.implementation.clone();
        config
    }

    /// Exactly `rmcp`'s default `ServerHandler::discover` (3.4.1,
    /// `handler/server.rs`), behind the caller check.
    pub(super) fn discovered(&self, extensions: &Extensions) -> Result<DiscoverResult, ErrorData> {
        self.caller.resolve_from(extensions)?;
        let versions = Cow::into_owned(self.supported_protocol_versions());
        Ok(DiscoverResult::from_server_info(versions, self.get_info()))
    }

    /// Nothing here is completed (no prompts, and no resource template
    /// variable offers values), so the answer is `rmcp`'s default, an empty
    /// list, behind the caller check.
    pub(super) fn completed(&self, extensions: &Extensions) -> Result<CompleteResult, ErrorData> {
        self.caller.resolve_from(extensions)?;
        Ok(CompleteResult::default())
    }
}
