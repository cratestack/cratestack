//! `@api_version` end to end on `transport rpc`: the RPC twin of
//! `api_version_client_round_trip.rs`, real generated `rpc_router` on a real
//! `TcpListener`, real generated RPC client, same schema.
//!
//! On RPC a procedure is addressed by its op id, `procedure.<name>`, and
//! `@api_version` does not enter it: procedure names are unique per schema
//! (the parser rejects duplicates), so the op id is already unambiguous, and
//! both the server's dispatch arm and every generated RPC client build the
//! same unversioned op id. This test pins that agreement for a versioned
//! procedure, so a future change that versions one side cannot ship without
//! the other.

mod server {
    cratestack::include_server_schema!("tests/fixtures/api_version_rpc.cstack", db = None);
}

mod client {
    cratestack::include_client_schema!("tests/fixtures/api_version_rpc.cstack");
}

use cratestack::{CratestackContext, CratestackError};
use cratestack_client_rust::{CborCodec, ClientConfig, CratestackClient};
use server::cratestack_schema as srv;

#[derive(Clone, Default)]
struct Procedures;

impl srv::procedures::ProcedureRegistry for Procedures {
    fn ping(
        &self,
        _db: &srv::Cratestack,
        _ctx: &CratestackContext,
        args: srv::procedures::ping::Args,
        _authorized: srv::procedures::ping::Authorized,
    ) -> impl core::future::Future<Output = Result<srv::procedures::ping::Output, CratestackError>> + Send
    {
        async move {
            Ok(srv::PingReply {
                echo: format!("v2:{}", args.args.message),
            })
        }
    }

    fn plain(
        &self,
        _db: &srv::Cratestack,
        _ctx: &CratestackContext,
        args: srv::procedures::plain::Args,
        _authorized: srv::procedures::plain::Authorized,
    ) -> impl core::future::Future<Output = Result<srv::procedures::plain::Output, CratestackError>> + Send
    {
        async move {
            Ok(srv::PingReply {
                echo: format!("plain:{}", args.args.message),
            })
        }
    }
}

#[derive(Clone)]
struct AllowAllAuth;

impl cratestack::AuthProvider for AllowAllAuth {
    type Error = CratestackError;

    fn authenticate(
        &self,
        _request: &cratestack::RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        core::future::ready(Ok(CratestackContext::authenticated([(
            "id".to_owned(),
            cratestack::Value::Int(1),
        )])))
    }
}

/// Serve the generated router on a real loopback listener and return a
/// generated client pointed at it.
async fn spawn() -> (
    client::cratestack_schema::client::Client<CborCodec>,
    tokio::task::JoinHandle<()>,
) {
    let router = srv::axum::rpc_router(
        srv::Cratestack::builder().build(),
        Procedures,
        (),
        cratestack_codec_cbor::CborCodec,
        AllowAllAuth,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        cratestack::axum::serve(listener, router).await.unwrap();
    });
    // `reqwest`'s `rustls-no-provider` feature needs a provider installed
    // before the first `Client` is built, even for plain `http://`.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let base_url = reqwest::Url::parse(&format!("http://{addr}")).unwrap();
    let runtime = CratestackClient::new(ClientConfig::new(base_url), CborCodec);
    (
        client::cratestack_schema::client::Client::new(runtime),
        server,
    )
}

fn args(message: &str) -> client::cratestack_schema::procedures::ping::Args {
    client::cratestack_schema::procedures::ping::Args {
        args: client::cratestack_schema::PingArgs {
            message: message.to_owned(),
        },
    }
}

#[tokio::test]
async fn generated_rpc_client_reaches_a_versioned_procedure() {
    let (client, server) = spawn().await;
    let reply = client.procedures().ping(&args("hello")).await;
    server.abort();
    let reply = reply.expect(
        "the generated RPC client and the rpc_router must agree on the op id \
         of a versioned procedure (procedure.ping)",
    );
    assert_eq!(reply.echo, "v2:hello");
}

#[tokio::test]
async fn generated_rpc_client_still_reaches_an_unversioned_procedure() {
    let (client, server) = spawn().await;
    let plain_args = client::cratestack_schema::procedures::plain::Args {
        args: args("hello").args,
    };
    let reply = client.procedures().plain(&plain_args).await;
    server.abort();
    assert_eq!(
        reply.expect("plain dispatches as procedure.plain").echo,
        "plain:hello"
    );
}
