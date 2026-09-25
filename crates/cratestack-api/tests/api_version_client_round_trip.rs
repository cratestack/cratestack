//! `@api_version` end to end on `transport rest`: the real generated server
//! (`include_server_schema!(db = None)`, bound to a real `TcpListener`) and
//! the real generated Rust client (`include_client_schema!`) from the same
//! schema must agree on a versioned procedure's path.
//!
//! The server mounts `ping`, declared `@api_version("v2")`, at
//! `/v2/$procs/ping`. The generated clients used to call `/$procs/ping`
//! instead, which the server never mounts, so every call missed with a 404.
//! `plain` is the unversioned control: it must keep working at
//! `/$procs/plain`, proving the fix did not version everything.
//!
//! The descriptor test pins the third place the path lives:
//! `ROUTE_TRANSPORTS`, which the REST idempotency and rate-limit resolvers
//! match `MatchedPath` against. It must name the path the router mounts.

mod server {
    cratestack::include_server_schema!("tests/fixtures/api_version.cstack", db = None);
}

mod client {
    cratestack::include_client_schema!("tests/fixtures/api_version.cstack");
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
    let router = srv::axum::router(
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
async fn generated_rest_client_reaches_a_versioned_procedure() {
    let (client, server) = spawn().await;
    let reply = client.procedures().ping(&args("hello"), &[]).await;
    server.abort();
    let reply = reply.expect(
        "the generated client must call the versioned route the server mounts \
         (/v2/$procs/ping), not the unversioned /$procs/ping",
    );
    assert_eq!(reply.echo, "v2:hello");
}

#[tokio::test]
async fn generated_rest_client_still_reaches_an_unversioned_procedure() {
    let (client, server) = spawn().await;
    let plain_args = client::cratestack_schema::procedures::plain::Args {
        args: args("hello").args,
    };
    let reply = client.procedures().plain(&plain_args, &[]).await;
    server.abort();
    assert_eq!(
        reply.expect("plain stays at /$procs/plain").echo,
        "plain:hello"
    );
}

#[test]
fn route_transport_descriptor_names_the_mounted_versioned_path() {
    let paths: Vec<&str> = srv::axum::ROUTE_TRANSPORTS
        .iter()
        .map(|route| route.path)
        .collect();
    assert!(
        paths.contains(&"/v2/$procs/ping"),
        "ROUTE_TRANSPORTS must carry the path the router mounts for ping, got {paths:?}"
    );
    assert!(paths.contains(&"/$procs/plain"), "got {paths:?}");
}
