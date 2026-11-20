//! A signed open of a `@frames(unsigned)` subscription over the generated
//! REST router and its envelope layer (cratestack#1378): `GET /$procs/<name>`
//! with the sealed message as the request body (which browsers cannot send;
//! Rust and native clients can) streams plain frames labelled
//! `Cratestack-Frames: unsigned`.

#[allow(dead_code)]
mod cbor_seq_support;
mod cose_support;
mod signed_subscription_support;

use std::sync::atomic::Ordering;
use std::time::Duration;

use cose_support::*;
use cratestack::axum::Router;
use cratestack::envelope_layer::EnvelopeMode;
use cratestack::{CratestackContext, CratestackError, SubscriptionStream, find_contract};
use cratestack_codec_cbor::CborCodec;
use signed_subscription_support::{ARGS, Hits, Target, endless_ticks};

cratestack::include_server_schema!("tests/fixtures/subscription_frames_rest.cstack", db = None);

use cratestack_schema::procedures::{hinted, ping, plain};

#[derive(Clone)]
struct Registry(Hits);

impl Registry {
    fn ticks(
        &self,
        to: i64,
    ) -> Result<SubscriptionStream<cratestack_schema::Tick>, CratestackError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        if to < 0 {
            return Ok(endless_ticks(|n| cratestack_schema::Tick { n }));
        }
        let ticks = (1..=to).rev().map(|n| Ok(cratestack_schema::Tick { n }));
        Ok(Box::pin(cratestack::futures::stream::iter(
            ticks.collect::<Vec<_>>(),
        )))
    }
}

impl cratestack_schema::procedures::ProcedureRegistry for Registry {
    async fn hinted(
        &self,
        _: &cratestack_schema::Cratestack,
        _: &CratestackContext,
        args: hinted::Args,
        _: hinted::Authorized,
    ) -> Result<SubscriptionStream<hinted::Output>, CratestackError> {
        self.ticks(args.args.to)
    }

    async fn plain(
        &self,
        _: &cratestack_schema::Cratestack,
        _: &CratestackContext,
        args: plain::Args,
        _: plain::Authorized,
    ) -> Result<SubscriptionStream<plain::Output>, CratestackError> {
        self.ticks(args.args.to)
    }

    async fn ping(
        &self,
        _: &cratestack_schema::Cratestack,
        _: &CratestackContext,
        args: ping::Args,
        _: ping::Authorized,
    ) -> Result<ping::Output, CratestackError> {
        Ok(cratestack_schema::Tick { n: args.args.to })
    }
}

fn router(allow: bool, hits: Hits, audience: &'static str) -> Router {
    router_life(allow, hits, audience, Duration::from_secs(3600))
}

fn router_life(allow: bool, hits: Hits, audience: &'static str, life: Duration) -> Router {
    router_with(allow, hits, audience, life, Default::default())
}

/// The router with `revocations` as its registry of live streams
/// (cratestack#1381), which the test revokes through.
fn router_revoking(hits: Hits, revocations: cratestack::StreamRevocations) -> Router {
    router_with(true, hits, AUDIENCE, Duration::from_secs(3600), revocations)
}

fn router_with(
    allow: bool,
    hits: Hits,
    audience: &'static str,
    life: Duration,
    revocations: cratestack::StreamRevocations,
) -> Router {
    let layer = cratestack_schema::axum::envelope_layer(
        server_envelope(),
        EnvelopeMode::Required,
        audience,
    )
    .max_signed_stream_life(life)
    .rest("", cratestack_schema::axum::ROUTE_TRANSPORTS)
    .build()
    .expect("layer");
    cratestack_schema::axum::router(
        cratestack_schema::Cratestack::builder()
            .with_stream_revocations(revocations)
            .build(),
        Registry(hits),
        (),
        CborCodec,
        signed_subscription_support::Caller(allow),
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
    .layer(layer)
}

fn target(op: &str, method: &'static str) -> Target {
    let path = format!("/$procs/{op}");
    Target {
        method,
        uri: format!("{path}?{ARGS}"),
        digest: *find_contract(cratestack_schema::OP_CONTRACTS, method, &path).expect("digest"),
        route: path,
        query: Some(ARGS),
        payload: Vec::new(),
    }
}

signed_subscription_cases!(router: router, target: target, methods: ["GET"]);
signed_subscription_stream_cases!(router: router, router_life: router_life, target: target, methods: ["GET"]);
signed_subscription_revocation_cases!(router: router_revoking, target: target, methods: ["GET"]);
