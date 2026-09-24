//! [`McpServer`]: `rmcp`'s [`ServerHandler`] over a generated tool table.
//!
//! No `#[tool]` macros: `rmcp` lets a handler answer `tools/list` and
//! `tools/call` from plain methods, and the table those answer from is data
//! the schema macro wrote (ADR 0002 § Dispatch, D4).

use std::borrow::Cow;

use cratestack_core::CratestackContext;
use cratestack_exec::{OpExecutor, StoreErrorPolicy};
use rmcp::model::{
    CacheScope, CallToolRequestParams, CallToolResponse, Implementation, ListPromptsResult,
    ListResourceTemplatesResult, ListResourcesResult, ListToolsResult, PaginatedRequestParams,
    ProtocolVersion, ReadResourceRequestParams, ReadResourceResponse, ResourcesCapability,
    ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler};

use crate::listing::{ToolTableError, build_listing};
use crate::streamable_http::caller::Caller;
use crate::table::McpTools;

/// The single version [`ServerHandler::supported_protocol_versions`]
/// returns. A `static` because the trait wants a `'static` slice.
static SUPPORTED: [ProtocolVersion; 1] = [ProtocolVersion::V_2026_07_28];

/// An MCP server over one schema's tools.
///
/// Over stdio it answers as one caller: the context is a constructor
/// argument with no default and no `Option` (ADR 0002 Q1), because stdio
/// has no transport-level identity, so the application states who the
/// caller is, deliberately — a `SystemContext::for_service(...)`, or
/// `CratestackContext::authenticated` from a token it verified. Over
/// Streamable HTTP each request brings its own, built by the application's
/// `AuthProvider` (`crate::streamable_http`). Either way every tool call runs under
/// exactly that context, through the procedure's generated policy check.
pub struct McpServer<T: McpTools> {
    pub(crate) tools: T,
    /// Who a call runs as. Read it through [`Caller::resolve`], never by
    /// matching on it: that is the one place the HTTP case fails closed.
    pub(crate) caller: Caller,
    pub(crate) executor: OpExecutor,
    /// What a failing rate-limit store does to a call. Held here, not on
    /// the executor, because applying it is the transport's job (it owns
    /// the log line and the result), exactly as `RateLimitLayer` holds its
    /// own on HTTP.
    pub(crate) store_error_policy: StoreErrorPolicy,
    listing: Vec<Tool>,
    implementation: Implementation,
}

impl<T: McpTools> McpServer<T> {
    /// Fails only when the table's schemas are not JSON objects or a name
    /// repeats, which the generated table never produces.
    pub fn new(tools: T, context: CratestackContext) -> Result<Self, ToolTableError> {
        Self::with_caller(tools, Caller::Fixed(Box::new(context)))
    }

    /// `pub(crate)`: only `crate::streamable_http` builds a server whose caller comes
    /// from the request, because only its guard puts one there.
    pub(crate) fn with_caller(tools: T, caller: Caller) -> Result<Self, ToolTableError> {
        let listing = build_listing(tools.tools())?;
        Ok(Self {
            tools,
            caller,
            // Nothing wired: `OpExecutor::new(None, _)` admits every call
            // and reserves nothing, the same as a REST router with neither
            // layer installed. The TTL is unread without a store.
            executor: OpExecutor::new(None, std::time::Duration::ZERO),
            store_error_policy: StoreErrorPolicy::default(),
            listing,
            implementation: Implementation::new("cratestack-mcp", env!("CARGO_PKG_VERSION")),
        })
    }

    /// Opt in to L3 admission: an executor built with an idempotency store
    /// and/or `with_rate_limit`, as the application would build one for
    /// `cratestack-axum`'s layers.
    pub fn with_executor(mut self, executor: OpExecutor) -> Self {
        self.executor = executor;
        self
    }

    /// Choose what a failing rate-limit store does to a call, as
    /// `RateLimitLayer::with_store_error_policy` does on HTTP; pass the
    /// same value to both. Defaults to [`StoreErrorPolicy::Allow`], HTTP's
    /// default: serve through a transport-class failure (`Unavailable`,
    /// including a lookup that outlives `DEFAULT_STORE_TIMEOUT`), refuse
    /// every other. [`StoreErrorPolicy::Deny`] refuses them all, for a
    /// limiter that is a security control rather than a capacity one.
    /// Unread without a rate limiter on the executor.
    pub fn with_store_error_policy(mut self, policy: StoreErrorPolicy) -> Self {
        self.store_error_policy = policy;
        self
    }

    /// The `serverInfo` `server/discover` reports. Defaults to this crate's
    /// own name and version.
    pub fn with_implementation(mut self, name: &str, version: &str) -> Self {
        self.implementation = Implementation::new(name, version);
        self
    }
}

impl<T: McpTools> ServerHandler for McpServer<T> {
    fn get_info(&self) -> ServerConfig {
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

    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Borrowed(&SUPPORTED)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        // The list is static and unfiltered, so the caller changes nothing
        // in it. Resolved anyway (maintainer decision on #1040) so that over
        // Streamable HTTP every list method, like the resource lists
        // (`resources::listed`) and `list_prompts`, answers only a request
        // the guard authenticated. Over stdio the caller is fixed and this
        // always succeeds.
        self.caller.resolve(&context)?;
        // The whole table in one page. `ttlMs: 0` and a private scope are
        // `rmcp`'s own `server/discover` defaults; the list is static, but a
        // redeploy can change it, and nothing here knows how often that is.
        let mut result = ListToolsResult::with_all_items(self.listing.clone());
        result.ttl_ms = Some(0);
        result.cache_scope = Some(CacheScope::Private);
        Ok(result)
    }

    /// No prompts, but a list method all the same: `rmcp`'s default would
    /// answer it below the guard, the one list `list_tools`' rule missed.
    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, ErrorData> {
        self.caller.resolve(&context)?;
        Ok(ListPromptsResult::default())
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let caller = self.caller.resolve(&context)?;
        crate::call::call_tool(self, &caller, request, &context.meta)
            .await
            .map(CallToolResponse::from)
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        use crate::resources::{list_resources, listed};
        listed(self, &context.extensions, list_resources)
    }

    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, ErrorData> {
        use crate::resources::{list_templates, listed};
        listed(self, &context.extensions, list_templates)
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        // The per-request caller, as `call_tool`'s: it is both whose rows
        // the read may see and whose rate-limit bucket it is charged to.
        let caller = self.caller.resolve(&context)?;
        crate::resources::read_resource(self, &caller, &request.uri)
            .await
            .map(ReadResourceResponse::from)
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.listing.iter().find(|tool| tool.name == name).cloned()
    }
}
