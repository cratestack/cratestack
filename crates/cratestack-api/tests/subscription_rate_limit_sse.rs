//! SSE subscription opens (`GET`/`POST /rpc/subscribe/{op}`) are charged once,
//! at open, by a `RateLimitLayer` wrapped around the generated router
//! (design `subscriptions-websocket.md` §6.7). No code path was added for it;
//! this pins the behaviour end to end.

use std::net::SocketAddr;
use std::sync::Arc;

use cratestack::axum::body::Body;
use cratestack::axum::extract::ConnectInfo;
use cratestack::axum::http::{Request, Response, StatusCode};
use cratestack::ratelimit::{InMemoryRateLimitStore, RateLimitConfig, RateLimitLayer};
use cratestack::{
    CratestackContext, CratestackError, RequestContext, SubscriptionStream, Value,
    include_server_schema,
};
use tower::util::ServiceExt;

include_server_schema!(
    "tests/fixtures/subscription_procedure_no_database.cstack",
    db = None
);

use cratestack_schema::procedures::{countdown, watch_room};

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

    async fn watch_room(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        _args: watch_room::Args,
        _authorized: watch_room::Authorized,
    ) -> Result<SubscriptionStream<watch_room::Output>, CratestackError> {
        Ok(Box::pin(cratestack::futures::stream::empty()))
    }
}

fn limited_router() -> cratestack::axum::Router {
    cratestack_schema::axum::rpc_router(
        cratestack_schema::Cratestack::builder().build(),
        Countdown,
        (),
        cratestack_codec_cbor::CborCodec,
        HeaderCaller,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
    .layer(
        RateLimitLayer::new(
            Arc::new(InMemoryRateLimitStore::default()),
            RateLimitConfig::new(1, 0.001),
        )
        .with_op_resolver(cratestack::idempotency::build_rpc_op_resolver(
            cratestack_schema::axum::OPS,
        )),
    )
}

fn with_peer(mut request: Request<Body>) -> Request<Body> {
    let peer: SocketAddr = "192.0.2.7:4000".parse().unwrap();
    request.extensions_mut().insert(ConnectInfo(peer));
    request
}

fn get() -> Request<Body> {
    with_peer(
        Request::get("/rpc/subscribe/procedure.countdown?args=%7B%22to%22%3A1%7D")
            .header("accept", "text/event-stream")
            .body(Body::empty())
            .unwrap(),
    )
}

fn post() -> Request<Body> {
    with_peer(
        Request::post("/rpc/subscribe/procedure.countdown")
            .header("accept", "text/event-stream")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"args":{"to":1}}"#))
            .unwrap(),
    )
}

async fn assert_throttled(response: Response<Body>) {
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(response.headers().contains_key("retry-after"));
    let body = cratestack::axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert!(
        String::from_utf8_lossy(&body).contains("resource_exhausted"),
        "{body:?}"
    );
}

#[tokio::test]
async fn rpc_sse_subscribe_get_is_charged_at_open() {
    let router = limited_router();
    let first = router.clone().oneshot(get()).await.unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    assert_throttled(router.oneshot(get()).await.unwrap()).await;
}

#[tokio::test]
async fn rpc_sse_subscribe_post_shares_the_bucket() {
    let router = limited_router();
    let first = router.clone().oneshot(get()).await.unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    assert_throttled(router.oneshot(post()).await.unwrap()).await;
}
