//! The REST test app: the generated server and client of one fixture.

#![allow(dead_code)]

pub mod server {
    cratestack::include_server_schema!("tests/fixtures/api_version.cstack", db = None);
}

pub mod client {
    cratestack::include_client_schema!("tests/fixtures/api_version.cstack");
}

use crate::cose_client_support::{Kind, client_envelope, layer, runtime, serve};
use cratestack::{CratestackContext, CratestackError, VerifiedSigner};
use cratestack_client_rust::CborCodec;
use server::cratestack_schema as srv;

#[derive(Clone, Default)]
struct Procedures;

impl srv::procedures::ProcedureRegistry for Procedures {
    fn ping(
        &self,
        _db: &srv::Cratestack,
        ctx: &CratestackContext,
        args: srv::procedures::ping::Args,
        _authorized: srv::procedures::ping::Authorized,
    ) -> impl core::future::Future<Output = Result<srv::procedures::ping::Output, CratestackError>> + Send
    {
        let signer = ctx.verified_signer().map(VerifiedSigner::alg);
        async move {
            Ok(srv::PingReply {
                echo: format!("v2:{}|signer={signer:?}", args.args.message),
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
pub struct AllowAllAuth;

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

/// The generated server behind the envelope layer, for `kind`.
pub async fn server(kind: Kind) -> std::net::SocketAddr {
    server_at(kind, None).await
}

/// The same server nested under `mount` (`"/api"`, `"/t/{tenant}"`), its
/// layer told where it is mounted.
pub async fn server_mounted(kind: Kind, mount: &'static str) -> std::net::SocketAddr {
    server_at(kind, Some(mount)).await
}

async fn server_at(kind: Kind, mount: Option<&'static str>) -> std::net::SocketAddr {
    let router = srv::axum::router(
        srv::Cratestack::builder().build(),
        Procedures,
        (),
        cratestack_codec_cbor::CborCodec,
        AllowAllAuth,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
    .layer(layer(kind, |envelope, policy, audience| {
        let builder = srv::axum::envelope_layer(envelope, policy, audience);
        match mount {
            Some(mount) => builder.mount_prefix(mount),
            None => builder,
        }
    }));
    match mount {
        Some(mount) => serve(cratestack::axum::Router::new().nest(mount, router)).await,
        None => serve(router).await,
    }
}

pub fn client_for(
    addr: std::net::SocketAddr,
    kind: Kind,
) -> client::cratestack_schema::client::Client<CborCodec> {
    client::cratestack_schema::client::Client::new(runtime(
        addr,
        client_envelope(kind, crate::cose_client_support::AUDIENCE),
    ))
}

pub fn args(message: &str) -> client::cratestack_schema::procedures::ping::Args {
    client::cratestack_schema::procedures::ping::Args {
        args: client::cratestack_schema::PingArgs {
            message: message.to_owned(),
        },
    }
}

/// One signed `ping`, as the failure tests see it.
pub async fn ping(
    client: &client::cratestack_schema::client::Client<CborCodec>,
    message: &str,
    headers: &[(&str, &str)],
) -> crate::cose_client_support::Outcome {
    match client.procedures().ping(&args(message), headers).await {
        Ok(reply) => crate::cose_client_support::Outcome::Ok(reply.echo),
        Err(cratestack_client_rust::ClientError::Envelope(error)) => {
            crate::cose_client_support::Outcome::from(error)
        }
        Err(other) => crate::cose_client_support::Outcome::Other(other.to_string()),
    }
}

/// The generated client of this fixture.
pub type Client = client::cratestack_schema::client::Client<CborCodec>;

/// A streamed call, which a signing client must refuse.
pub async fn stream(client: &Client) -> crate::cose_client_support::Outcome {
    match client
        .runtime()
        .post_list_streamed::<_, client::cratestack_schema::PingReply>(
            "/v2/$procs/ping",
            &args("a").args,
            &[],
        )
        .await
    {
        Ok(_) => crate::cose_client_support::Outcome::Other("a stream was opened".to_owned()),
        Err(cratestack_client_rust::ClientError::Envelope(error)) => {
            crate::cose_client_support::Outcome::from(error)
        }
        Err(other) => crate::cose_client_support::Outcome::Other(other.to_string()),
    }
}

/// As [`server`], but taking JSON as well as CBOR, and with the layer
/// allowing both inside the seal (cratestack#1168).
pub async fn server_json(kind: Kind) -> std::net::SocketAddr {
    let router = srv::axum::router(
        srv::Cratestack::builder().build(),
        Procedures,
        (),
        cratestack::CodecSet::new(cratestack_codec_cbor::CborCodec, cratestack::JsonCodec),
        AllowAllAuth,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
    .layer(layer(kind, |envelope, policy, audience| {
        srv::axum::envelope_layer(envelope, policy, audience).payload_media_types(
            crate::cose_client_support::CBOR_AND_JSON,
            crate::cose_client_support::CBOR_AND_JSON,
        )
    }));
    serve(router).await
}

/// The generated client with a JSON codec, sealing JSON.
pub fn json_client_for(
    addr: std::net::SocketAddr,
    kind: Kind,
) -> client::cratestack_schema::client::Client<cratestack_client_rust::JsonCodec> {
    client::cratestack_schema::client::Client::new(crate::cose_client_support::runtime_with(
        addr,
        cratestack_client_rust::JsonCodec,
        client_envelope(kind, crate::cose_client_support::AUDIENCE),
    ))
}
