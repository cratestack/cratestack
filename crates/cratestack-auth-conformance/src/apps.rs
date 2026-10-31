//! The kit's two generated schemas and the routers it builds from them: REST
//! and MCP from `schemas/rest.cstack`, RPC from `schemas/rpc.cstack`.

use cratestack::AuthProvider;
use cratestack::axum::Router;
use cratestack::mcp::{CredentialPresence, ProtectedResource, StreamableHttpServer};
use cratestack_codec_json::JsonCodec;

/// The MCP resource identifier; its path is the canonical MCP path.
pub(crate) const MCP_RESOURCE: &str = "http://localhost/mcp";
const MCP_ISSUER: &str = "https://issuer.conformance.invalid";

/// Who the caller is, as the procedure reports it: the framework's own
/// answer, [`CratestackContext::subject`](cratestack::CratestackContext::subject).
pub(crate) fn subject(ctx: &cratestack::CratestackContext) -> String {
    ctx.subject().map(String::from).unwrap_or_default()
}

mod rest {
    use cratestack::{CratestackContext, CratestackError, include_server_schema};

    include_server_schema!("schemas/rest.cstack", db = None);

    pub(crate) use cratestack_schema::*;

    #[derive(Clone, Default)]
    pub(crate) struct Registry;

    impl cratestack_schema::procedures::ProcedureRegistry for Registry {
        async fn who_am_i(
            &self,
            _db: &cratestack_schema::Cratestack,
            ctx: &CratestackContext,
            _args: cratestack_schema::procedures::who_am_i::Args,
            _authorized: cratestack_schema::procedures::who_am_i::Authorized,
        ) -> Result<cratestack_schema::Identity, CratestackError> {
            Ok(cratestack_schema::Identity {
                subject: super::subject(ctx),
            })
        }
    }

    #[derive(Clone)]
    pub(crate) struct Resolver;

    impl cratestack_schema::ComputedFieldResolver for Resolver {}
}

mod rpc {
    use cratestack::{CratestackContext, CratestackError, include_server_schema};

    include_server_schema!("schemas/rpc.cstack", db = None);

    pub(crate) use cratestack_schema::*;

    #[derive(Clone, Default)]
    pub(crate) struct Registry;

    impl cratestack_schema::procedures::ProcedureRegistry for Registry {
        async fn who_am_i(
            &self,
            _db: &cratestack_schema::Cratestack,
            ctx: &CratestackContext,
            _args: cratestack_schema::procedures::who_am_i::Args,
            _authorized: cratestack_schema::procedures::who_am_i::Authorized,
        ) -> Result<cratestack_schema::Identity, CratestackError> {
            Ok(cratestack_schema::Identity {
                subject: super::subject(ctx),
            })
        }
    }
}

/// The REST router, nested under `mount` (`""` is the root).
pub(crate) fn rest<A: AuthProvider>(auth: A, mount: &str) -> Router {
    let router = rest::axum::router(
        rest::Cratestack::builder().build(),
        rest::Registry,
        (),
        JsonCodec,
        auth,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    );
    nest(router, mount)
}

/// The RPC router (unary and batch), nested under `mount`.
pub(crate) fn rpc<A: AuthProvider>(auth: A, mount: &str) -> Router {
    let router = rpc::axum::rpc_router(
        rpc::Cratestack::builder().build(),
        rpc::Registry,
        (),
        JsonCodec,
        auth,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    );
    nest(router, mount)
}

/// The MCP Streamable HTTP endpoint at `/mcp`, where the provider's own
/// `credential` decides presence (`CredentialPresence::ask_provider()`).
pub(crate) fn mcp<A: AuthProvider>(auth: A) -> Router {
    let tools = rest::mcp::tools(
        rest::Cratestack::builder().build(),
        rest::Registry,
        rest::Resolver,
    );
    let server = StreamableHttpServer::builder(
        tools,
        auth,
        ["http://localhost"],
        ProtectedResource::new(MCP_RESOURCE, [MCP_ISSUER]),
    )
    .with_credential_presence(CredentialPresence::ask_provider())
    .build()
    .expect("the kit's MCP configuration is valid");
    Router::new().nest_service("/mcp", server.service())
}

fn nest(router: Router, mount: &str) -> Router {
    if mount.is_empty() {
        router
    } else {
        Router::new().nest(mount, router)
    }
}
