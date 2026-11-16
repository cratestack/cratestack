//! The REST test app: the generated server and client of the frames fixture.

#![allow(dead_code)]

pub mod server {
    cratestack::include_server_schema!("tests/fixtures/subscription_frames_rest.cstack", db = None);
}

pub mod client {
    cratestack::include_client_schema!("tests/fixtures/subscription_frames_rest.cstack");
}

use std::net::SocketAddr;
use std::sync::atomic::Ordering;
use std::time::Duration;

use cratestack::{CratestackContext, CratestackError, SubscriptionStream};
use cratestack_client_rust::{CborCodec, SubscriptionTransport};
use server::cratestack_schema as srv;
use srv::procedures::{hinted, ping, plain};

use crate::cose_client_stream_support::{Caller, Drained, Fail, Hits, drain, endless_ticks};
use crate::cose_client_support::{
    AUDIENCE, Kind, client_envelope, layer, runtime_subscribing, serve,
};

const KIND: Kind = Kind::Ed25519;

#[derive(Clone)]
struct Registry(Hits);

impl Registry {
    fn ticks(&self, to: i64) -> Result<SubscriptionStream<srv::Tick>, CratestackError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        if to < 0 {
            return Ok(endless_ticks(|n| srv::Tick { n }));
        }
        let ticks = (1..=to).rev().map(|n| Ok(srv::Tick { n }));
        Ok(Box::pin(cratestack::futures::stream::iter(
            ticks.collect::<Vec<_>>(),
        )))
    }
}

impl srv::procedures::ProcedureRegistry for Registry {
    async fn hinted(
        &self,
        _: &srv::Cratestack,
        _: &CratestackContext,
        args: hinted::Args,
        _: hinted::Authorized,
    ) -> Result<SubscriptionStream<hinted::Output>, CratestackError> {
        self.ticks(args.args.to)
    }

    async fn plain(
        &self,
        _: &srv::Cratestack,
        _: &CratestackContext,
        args: plain::Args,
        _: plain::Authorized,
    ) -> Result<SubscriptionStream<plain::Output>, CratestackError> {
        self.ticks(args.args.to)
    }

    async fn ping(
        &self,
        _: &srv::Cratestack,
        _: &CratestackContext,
        args: ping::Args,
        _: ping::Authorized,
    ) -> Result<ping::Output, CratestackError> {
        Ok(srv::Tick { n: args.args.to })
    }
}

/// The generated server behind the envelope layer: `allow` is whether its
/// auth provider authenticates, `life` its `max_signed_stream_life`.
pub async fn server(allow: bool, life: Duration) -> (SocketAddr, Hits) {
    let hits = Hits::default();
    let router = srv::axum::router(
        srv::Cratestack::builder().build(),
        Registry(hits.clone()),
        (),
        cratestack_codec_cbor::CborCodec,
        Caller(allow),
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
    .layer(layer(KIND, |envelope, policy, audience| {
        srv::axum::envelope_layer(envelope, policy, audience).max_signed_stream_life(life)
    }));
    (serve(router).await, hits)
}

/// The generated client of this fixture.
pub type Client = client::cratestack_schema::client::Client<CborCodec>;

/// A signing client of `addr` opening subscriptions over `transport`.
pub fn client(addr: SocketAddr, transport: SubscriptionTransport) -> Client {
    Client::new(runtime_subscribing(
        addr,
        client_envelope(KIND, AUDIENCE),
        transport,
    ))
}

/// Opens the marked subscription `hinted` and reads it to its end.
pub async fn hinted_ticks(client: &Client, to: i64) -> Result<Drained, Fail> {
    let args = client::cratestack_schema::procedures::hinted::Args {
        args: client::cratestack_schema::CountArgs { to },
    };
    let subscription = client.procedures().hinted(&args).await?;
    Ok(drain(subscription, |tick| tick.n).await)
}

/// As [`hinted_ticks`], for the unmarked `plain`.
pub async fn plain_ticks(client: &Client, to: i64) -> Result<Drained, Fail> {
    let args = client::cratestack_schema::procedures::plain::Args {
        args: client::cratestack_schema::CountArgs { to },
    };
    let subscription = client.procedures().plain(&args).await?;
    Ok(drain(subscription, |tick| tick.n).await)
}
