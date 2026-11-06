//! A `subscription procedure` under `db = None` (slice S4): no database, the
//! handler's own stream is the subscription, served over `/rpc/subscribe` and
//! ended when the handler's stream ends, or a topic on the in-process
//! backplane that the handler forwards.

use cratestack::axum::body::Body;
use cratestack::axum::http::{Request, StatusCode};
use cratestack::futures::StreamExt;
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
        request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        let authenticated = request.headers.contains_key("x-caller");
        core::future::ready(Ok(if authenticated {
            CratestackContext::authenticated([("id".to_owned(), Value::Int(1))])
        } else {
            CratestackContext::anonymous()
        }))
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
        let ticks = (1..=args.args.to)
            .rev()
            .map(|n| Ok(cratestack_schema::Tick { n }));
        Ok(Box::pin(cratestack::futures::stream::iter(
            ticks.collect::<Vec<_>>(),
        )))
    }

    async fn watch_room(
        &self,
        db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        args: watch_room::Args,
        _authorized: watch_room::Authorized,
    ) -> Result<SubscriptionStream<watch_room::Output>, CratestackError> {
        db.topics().room_tick(&args.args.roomId).subscribe().await
    }
}

#[tokio::test]
async fn the_handlers_stream_is_the_subscription() {
    let router = cratestack_schema::axum::rpc_router(
        cratestack_schema::Cratestack::builder().build(),
        Countdown,
        (),
        cratestack_codec_cbor::CborCodec,
        HeaderCaller,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    );
    let request = |caller: bool| {
        let mut request =
            Request::get("/rpc/subscribe/procedure.countdown?args=%7B%22to%22%3A3%7D")
                .header("accept", "text/event-stream");
        if caller {
            request = request.header("x-caller", "1");
        }
        request.body(Body::empty()).unwrap()
    };
    let refused = router.clone().oneshot(request(false)).await.unwrap();
    assert_eq!(refused.status(), StatusCode::FORBIDDEN);
    let response = router.oneshot(request(true)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    // The handler's stream ends, so the response does: no terminal error.
    let body = cratestack::axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("the stream ends");
    let text = String::from_utf8(body.to_vec()).unwrap();
    let ticks: Vec<&str> = text
        .split("\n\n")
        .filter_map(|event| event.strip_prefix("event: message\ndata: "))
        .collect();
    assert_eq!(
        ticks,
        [
            r#"{"id":2,"next":{"n":3}}"#,
            r#"{"id":3,"next":{"n":2}}"#,
            r#"{"id":4,"next":{"n":1}}"#,
        ],
        "{text}"
    );
    assert!(!text.contains("event: error"), "{text}");
}

fn watch(room: &str) -> Request<Body> {
    Request::get(format!(
        "/rpc/subscribe/procedure.watchRoom?args=%7B%22roomId%22%3A%22{room}%22%7D"
    ))
    .header("accept", "text/event-stream")
    .header("x-caller", "1")
    .body(Body::empty())
    .unwrap()
}

fn router(db: cratestack_schema::Cratestack) -> cratestack::axum::Router {
    cratestack_schema::axum::rpc_router(
        db,
        Countdown,
        (),
        cratestack_codec_cbor::CborCodec,
        HeaderCaller,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
}

/// What the response sends within a short window: `ready`, then any messages.
async fn events(response: cratestack::axum::response::Response) -> String {
    assert_eq!(response.status(), StatusCode::OK);
    let mut body = response.into_body().into_data_stream();
    let mut text = String::new();
    while let Ok(Some(chunk)) =
        tokio::time::timeout(std::time::Duration::from_millis(300), body.next()).await
    {
        text.push_str(&String::from_utf8_lossy(&chunk.expect("body chunk")));
    }
    assert!(text.contains("event: ready"), "{text}");
    text
}

/// The default backplane is shared by every `build()` in the process, so a
/// handle built elsewhere (a worker, another router) reaches the subscribers.
#[tokio::test]
async fn a_topic_reaches_its_keys_subscribers_across_builds() {
    let router = router(cratestack_schema::Cratestack::builder().build());
    // The response arrives once the subscription is live, so what is
    // published next cannot be missed.
    let r1 = router.clone().oneshot(watch("r1")).await.unwrap();
    let r2 = router.oneshot(watch("r2")).await.unwrap();

    cratestack_schema::Cratestack::builder()
        .build()
        .topics()
        .room_tick("r1")
        .publish(&cratestack_schema::Tick { n: 7 })
        .await
        .unwrap();

    let r1 = events(r1).await;
    assert!(r1.contains(r#""next":{"n":7}"#), "{r1}");
    let r2 = events(r2).await;
    assert!(
        !r2.contains("event: message"),
        "r2 received r1's tick: {r2}"
    );
}

#[tokio::test]
async fn with_backplane_replaces_the_shared_default() {
    let own = std::sync::Arc::new(cratestack::InProcessBackplane::new());
    let db = cratestack_schema::Cratestack::builder()
        .with_backplane(own)
        .build();
    let subscribed = router(db.clone()).oneshot(watch("own")).await.unwrap();

    // The shared default does not reach it; its own backplane does.
    cratestack_schema::Cratestack::builder()
        .build()
        .topics()
        .room_tick("own")
        .publish(&cratestack_schema::Tick { n: 1 })
        .await
        .unwrap();
    db.topics()
        .room_tick("own")
        .publish(&cratestack_schema::Tick { n: 2 })
        .await
        .unwrap();

    let text = events(subscribed).await;
    assert!(text.contains(r#""next":{"n":2}"#), "{text}");
    assert!(!text.contains(r#""next":{"n":1}"#), "{text}");
}
