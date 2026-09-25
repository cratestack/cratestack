//! [`StreamableHttpServer`]: the builder, and [`StreamableHttp`], what it
//! builds.
//!
//! The allowed-origins list and the `AuthProvider` are constructor
//! arguments, not setters, so a server without them cannot be written
//! (cratestack#1039). An empty origins list compiles but does not build:
//! `rmcp` reads an empty list as "do not check `Origin`".

use std::sync::Arc;

use cratestack_core::AuthProvider;
use cratestack_exec::{OpExecutor, StoreErrorPolicy};
use rmcp::transport::StreamableHttpServerConfig;
use rmcp::transport::streamable_http_server::session::never::NeverSessionManager;

use super::caller::Caller;
use super::error::HttpConfigError;
use super::guard::{MAX_BODY_BYTES, Shared, StreamableHttpService};
use super::origin::AllowedOrigins;
use super::resource::ProtectedResource;
use crate::server::McpServer;
use crate::table::McpTools;

/// Builds an MCP server over Streamable HTTP. See the module doc of
/// `cratestack_mcp::streamable_http` for the mounting recipe.
pub struct StreamableHttpServer<T: McpTools, A: AuthProvider> {
    tools: T,
    provider: A,
    allowed_origins: Vec<String>,
    resource: ProtectedResource,
    allowed_hosts: Option<Vec<String>>,
    executor: Option<OpExecutor>,
    store_error_policy: Option<StoreErrorPolicy>,
    implementation: Option<(String, String)>,
}

impl<T: McpTools, A: AuthProvider> StreamableHttpServer<T, A> {
    /// `provider` authenticates every request, and should check that a
    /// token's audience is `resource`'s identifier: MCP requires it, and
    /// nothing else here can. `allowed_origins` are the browser origins
    /// that may call the endpoint (`scheme://host[:port]`); it may not be
    /// empty.
    pub fn builder(
        tools: T,
        provider: A,
        allowed_origins: impl IntoIterator<Item = impl Into<String>>,
        resource: ProtectedResource,
    ) -> Self {
        Self {
            tools,
            provider,
            allowed_origins: allowed_origins.into_iter().map(Into::into).collect(),
            resource,
            allowed_hosts: None,
            executor: None,
            store_error_policy: None,
            implementation: None,
        }
    }

    /// The `Host` values accepted (`host` or `host:port`), `rmcp`'s DNS
    /// rebinding guard. Defaults to the resource identifier's own
    /// `host[:port]`: `rmcp`'s default, loopback only, would refuse every
    /// request to a deployed service, and the identifier already names the
    /// host clients are meant to use. Set this when a proxy rewrites `Host`.
    pub fn with_allowed_hosts(
        mut self,
        hosts: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.allowed_hosts = Some(hosts.into_iter().map(Into::into).collect());
        self
    }

    /// L3 admission, exactly as over stdio. See [`McpServer::with_executor`].
    pub fn with_executor(mut self, executor: OpExecutor) -> Self {
        self.executor = Some(executor);
        self
    }

    /// See [`McpServer::with_store_error_policy`].
    pub fn with_store_error_policy(mut self, policy: StoreErrorPolicy) -> Self {
        self.store_error_policy = Some(policy);
        self
    }

    /// See [`McpServer::with_implementation`].
    pub fn with_implementation(mut self, name: &str, version: &str) -> Self {
        self.implementation = Some((name.to_owned(), version.to_owned()));
        self
    }

    pub fn build(self) -> Result<StreamableHttp<T, A>, HttpConfigError> {
        let origins = AllowedOrigins::new(self.allowed_origins)?;
        let resource = self.resource.resolve()?;
        let hosts = self
            .allowed_hosts
            .unwrap_or_else(|| vec![resource.authority.clone()]);
        if let Some(empty) = hosts.iter().find(|host| host.trim().is_empty()) {
            return Err(HttpConfigError::InvalidHost(empty.clone()));
        }

        let mut server = McpServer::with_caller(self.tools, Caller::PerRequest)?;
        if let Some(executor) = self.executor {
            server = server.with_executor(executor);
        }
        if let Some(policy) = self.store_error_policy {
            server = server.with_store_error_policy(policy);
        }
        if let Some((name, version)) = &self.implementation {
            server = server.with_implementation(name, version);
        }
        let server = Arc::new(server);

        // Stateless (2026-07-28 has no sessions), JSON answers for a
        // terminal message, per-request protocol metadata required, and the
        // Origin list enforced even if it were empty: the guard's check is
        // the first wall, this is the second (defence in depth).
        let config = StreamableHttpServerConfig::default()
            .with_legacy_session_mode(false)
            .with_json_response(true)
            .with_stateless_protocol_metadata_required(true)
            .with_max_request_body_bytes(MAX_BODY_BYTES)
            .with_allowed_hosts(hosts)
            .with_allowed_origins(origins.for_rmcp())
            .enforce_origin_validation();
        let inner = rmcp::transport::StreamableHttpService::new(
            move || Ok(Arc::clone(&server)),
            Arc::new(NeverSessionManager::default()),
            config,
        );
        Ok(StreamableHttp {
            service: StreamableHttpService {
                shared: Arc::new(Shared {
                    inner,
                    provider: self.provider,
                    origins,
                    resource,
                }),
            },
        })
    }
}

/// A built server: the endpoint and its metadata document.
pub struct StreamableHttp<T: McpTools, A: AuthProvider> {
    service: StreamableHttpService<T, A>,
}

impl<T: McpTools, A: AuthProvider> StreamableHttp<T, A> {
    /// The MCP endpoint. Mount it where the resource identifier says it
    /// is, for example `.nest_service("/mcp", http.service())`.
    pub fn service(&self) -> StreamableHttpService<T, A> {
        self.service.clone()
    }

    /// RFC 9728 metadata at `/.well-known/oauth-protected-resource` plus
    /// the path-suffixed form for the resource's path. Merge it into the
    /// application's **root** router (`.merge(http.metadata_router())`),
    /// never under a `nest`: RFC 9728 puts the document at the host root,
    /// whatever path the endpoint is mounted at.
    pub fn metadata_router(&self) -> axum::Router {
        super::metadata::router(&self.service.shared.resource)
    }

    /// The absolute URL every 401 challenge names as `resource_metadata`.
    pub fn resource_metadata_url(&self) -> &str {
        &self.service.shared.resource.metadata_url
    }
}
