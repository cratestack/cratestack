//! A REST subscription procedure (`GET /$procs/<name>`, SSE) is charged once,
//! at open, by a `RateLimitLayer` wrapped around the generated router
//! (design `subscriptions-websocket.md` §6.7). Its own binary: one
//! `include_server_schema!` per test crate.

use std::net::SocketAddr;
use std::sync::Arc;

use cratestack::axum::body::Body;
use cratestack::axum::extract::ConnectInfo;
use cratestack::axum::http::{Request, StatusCode};
use cratestack::ratelimit::{InMemoryRateLimitStore, RateLimitConfig, RateLimitLayer};
use cratestack::{
    CratestackContext, CratestackError, RequestContext, SubscriptionStream, Value,
    include_server_schema,
};
use tower::util::ServiceExt;

include_server_schema!(
    "tests/fixtures/subscription_procedure_rest_no_database.cstack",
    db = None
);

use cratestack_schema::procedures::countdown;

#[derive(Clone)]
struct HeaderCaller;

impl cratestack::AuthProvider for HeaderCaller {
    type Error = CratestackError;

    fn authenticate(
        &self,
        _request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        core::future::ready(Ok(CratestackContext::authenticated([(
            "id".to_owned(),
            Value::Int(1),
        )])))
    }
}

#[derive(Clone)]
struct Countdown;

impl cratestack_schema::procedures::ProcedureRegistry for Countdown {
    async fn countdown(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        args: countdown::Args,
        _authorized: countdown::Authorized,
    ) -> Result<SubscriptionStream<countdown::Output>, CratestackError> {
        let ticks = (1..=args.args.to).map(|n| Ok(cratestack_schema::Tick { n }));
        Ok(Box::pin(cratestack::futures::stream::iter(
            ticks.collect::<Vec<_>>(),
        )))
    }
}

fn open() -> Request<Body> {
    let mut request = Request::get("/$procs/countdown?args=%7B%22to%22%3A1%7D")
        .header("accept", "text/event-stream")
        .body(Body::empty())
        .unwrap();
    let peer: SocketAddr = "192.0.2.7:4000".parse().unwrap();
    request.extensions_mut().insert(ConnectInfo(peer));
    request
}

#[tokio::test]
async fn rest_procs_subscription_is_charged_at_open() {
    let layer = RateLimitLayer::new(
        Arc::new(InMemoryRateLimitStore::default()),
        RateLimitConfig::new(1, 0.001),
    )
    .with_op_resolver(cratestack::idempotency::build_rest_op_resolver(
        cratestack_schema::axum::ROUTE_TRANSPORTS,
    ));
    let router = cratestack_schema::axum::router(
        cratestack_schema::Cratestack::builder().build(),
        Countdown,
        (),
        cratestack_codec_cbor::CborCodec,
        HeaderCaller,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
    .route_layer(layer);

    let first = router.clone().oneshot(open()).await.unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    let second = router.oneshot(open()).await.unwrap();
    assert_eq!(second.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(second.headers().contains_key("retry-after"));
}
