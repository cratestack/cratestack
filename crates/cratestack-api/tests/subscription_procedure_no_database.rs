//! A `subscription procedure` under `db = None` (slice S4): no database, no
//! backplane, the handler's own stream is the subscription, served over
//! `/rpc/subscribe` and ended when the handler's stream ends.

use cratestack::axum::body::Body;
use cratestack::axum::http::{Request, StatusCode};
use cratestack::{
    CratestackContext, CratestackError, RequestContext, SubscriptionStream, Value,
    include_server_schema,
};
use tower::util::ServiceExt;

include_server_schema!(
    "tests/fixtures/subscription_procedure_no_database.cstack",
    db = None
);

use cratestack_schema::procedures::countdown;

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
