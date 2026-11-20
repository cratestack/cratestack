//! Ending a live subscription when its principal is revoked (cratestack#1381),
//! over RPC (`GET /rpc/subscribe/procedure.ticks`), with the generated
//! router, no database.

use cratestack::{CratestackContext, CratestackError, SubscriptionStream, include_server_schema};

include_server_schema!(
    "tests/fixtures/subscription_revocation_rpc.cstack",
    db = None,
    decimal = RustDecimal
);

mod revocation_support;

use cratestack_schema::procedures::ticks;
use revocation_support::Caller;

const PATH: &str = "/rpc/subscribe/procedure.ticks?args=%7B%22count%22%3A0%7D";

#[derive(Clone)]
struct Ticks;

impl cratestack_schema::procedures::ProcedureRegistry for Ticks {
    async fn ticks(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        _args: ticks::Args,
        _authorized: ticks::Authorized,
    ) -> Result<SubscriptionStream<ticks::Output>, CratestackError> {
        Ok(Box::pin(cratestack::futures::stream::pending()))
    }
}

fn setup() -> (cratestack::axum::Router, cratestack::StreamRevocations) {
    let db = cratestack_schema::Cratestack::builder().build();
    let revocations = db.stream_revocations().clone();
    let router = cratestack_schema::axum::rpc_router(
        db,
        Ticks,
        (),
        cratestack_codec_cbor::CborCodec,
        Caller,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    );
    (router, revocations)
}

#[tokio::test]
async fn a_session_end_ends_a_live_cbor_seq_stream() {
    let (router, revocations) = setup();
    revocation_support::session_ended_ends_a_cbor_seq_stream(&router, &revocations, PATH).await;
}

#[tokio::test]
async fn a_suspension_ends_a_live_sse_stream() {
    let (router, revocations) = setup();
    revocation_support::suspension_ends_an_sse_stream(&router, &revocations, PATH).await;
}

#[tokio::test]
async fn a_revoke_is_scoped_and_a_reopen_with_the_cached_context_is_refused() {
    let (router, revocations) = setup();
    revocation_support::a_revoke_is_scoped_and_refuses_a_reopen(&router, &revocations, PATH).await;
}

#[tokio::test]
async fn an_expired_credential_ends_with_its_typed_reason() {
    let (router, _) = setup();
    revocation_support::an_expired_credential_ends_with_its_reason(&router, PATH).await;
}

/// The generated dispatcher hands the bindings the runtime's registry, so a
/// WebSocket or WebTransport session is watched in the registry `revoke` is
/// called on.
#[tokio::test]
async fn the_generated_dispatcher_exposes_the_runtimes_registry() {
    use cratestack::SubscriptionDispatcher;

    let db = cratestack_schema::Cratestack::builder().build();
    let dispatcher = db.subscriptions(Ticks);
    let registry = dispatcher.revocations().expect("a registry");
    let watch = registry
        .watch(vec![cratestack::RevocationKey::Session("s".into())])
        .unwrap();
    let ended = db.stream_revocations().revoke(
        cratestack::RevocationKey::Session("s".into()),
        cratestack::StreamEndReason::SessionEnded,
        None,
    );
    assert_eq!(ended, 1);
    assert!(watch.try_ended().is_some());
}
