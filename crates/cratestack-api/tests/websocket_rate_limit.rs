//! The generated dispatcher over WebSocket with a rate limit
//! (`WebSocketConfig::rate_limit(layer.for_subscriptions())`, design
//! `subscriptions-websocket.md` §6.7): each open costs a token, a throttle
//! reaches the generated client as a 429-class error, and `Auto` does not
//! fall back to SSE on it. The router is not wrapped in the layer, so only
//! the per-`Request` admission is exercised.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use cratestack::client_rust::{
    ClientConfig, ClientError, CratestackClient, RpcClientError, SubscriptionTransport,
};
use cratestack::ratelimit::{InMemoryRateLimitStore, RateLimitConfig, RateLimitLayer};
use cratestack::websocket::WebSocketConfig;
use cratestack::{
    AuthProvider, CratestackContext, CratestackError, RequestContext, SubscriptionStream, Value,
    include_server_schema,
};
use cratestack_codec_cbor::CborCodec;

include_server_schema!("tests/fixtures/websocket_rate_limit.cstack", db = None);

use cratestack_schema::procedures::countdown;

#[derive(Clone)]
struct BearerOne;

impl AuthProvider for BearerOne {
    type Error = CratestackError;

    fn authenticate(
        &self,
        request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        core::future::ready(if request.headers.contains_key("authorization") {
            Ok(CratestackContext::authenticated([(
                "id".to_owned(),
                Value::Int(1),
            )]))
        } else {
            Err(CratestackError::Unauthorized("no credentials".to_owned()))
        })
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

struct AsCallerOne;

#[async_trait::async_trait]
impl cratestack::client_rust::RequestAuthorizer for AsCallerOne {
    async fn authorize(
        &self,
        _request: &cratestack::client_rust::AuthorizationRequest,
    ) -> Result<Vec<(String, String)>, ClientError> {
        Ok(vec![("authorization".to_owned(), "Bearer 1".to_owned())])
    }
}

/// A server whose WebSocket opens share a bucket of `burst` tokens. The SSE
/// route is unlayered, so a client that fell back to it would succeed.
async fn serve(burst: u32) -> String {
    let layer = RateLimitLayer::new(
        Arc::new(InMemoryRateLimitStore::default()),
        RateLimitConfig::new(burst, 0.001),
    );
    let db = cratestack_schema::Cratestack::builder().build();
    let app = cratestack_schema::axum::rpc_router(
        db,
        Countdown,
        (),
        CborCodec,
        BearerOne,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
    .merge(db.subscriptions(Countdown).websocket(
        WebSocketConfig::new().rate_limit(layer.for_subscriptions()),
        BearerOne,
    ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/", listener.local_addr().unwrap());
    let app = app.into_make_service_with_connect_info::<SocketAddr>();
    tokio::spawn(async move { cratestack::axum::serve(listener, app).await.unwrap() });
    base
}

fn client(base: &str, transport: SubscriptionTransport) -> cratestack_schema::client::Client {
    let config = ClientConfig::new(base.parse().unwrap()).with_subscription_transport(transport);
    cratestack_schema::client::Client::new(
        CratestackClient::cbor(config).with_request_authorizer(Arc::new(AsCallerOne)),
    )
}

fn args() -> countdown::Args {
    countdown::Args {
        args: cratestack_schema::CountArgs { to: 2 },
    }
}

fn assert_throttle(error: RpcClientError) {
    let RpcClientError::Remote(remote) = error else {
        panic!("expected a refusal, got {error:?}");
    };
    assert_eq!(remote.status, 429);
    // Not `body.code`: the client's `err`-frame conversion re-maps the code
    // through the REST envelope's names and reads a lowercase RPC code as
    // `internal` (pre-existing, every binding). The status and `details`
    // survive, and are what a backoff keys on.
    let retry = remote
        .body
        .details
        .as_ref()
        .and_then(|d| d.get("retry_after_secs"));
    assert!(retry.is_some(), "{:?}", remote.body);
}

#[tokio::test]
async fn the_generated_client_is_throttled_over_websocket() {
    let base = serve(1).await;
    let client = client(&base, SubscriptionTransport::WebSocket);
    let mut ticks = client
        .procedures()
        .countdown(&args())
        .await
        .expect("the first open is charged and admitted");
    let first = tokio::time::timeout(Duration::from_secs(5), ticks.recv())
        .await
        .unwrap();
    assert!(first.is_some(), "the admitted subscription yields ticks");

    let refused = client.procedures().countdown(&args()).await;
    assert_throttle(refused.map(drop).expect_err("the second open is throttled"));
}

#[tokio::test]
async fn auto_does_not_fall_back_on_a_throttle() {
    let base = serve(0).await;
    let refused = client(&base, SubscriptionTransport::Auto)
        .procedures()
        .countdown(&args())
        .await;
    assert_throttle(
        refused
            .map(drop)
            .expect_err("a throttle is a refusal, not a fallback"),
    );
}
