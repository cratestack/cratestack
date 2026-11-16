//! A REST subscription procedure (`GET /$procs/<name>`) refuses a wrong
//! `Accept` with `406` *before* it authenticates the caller (cratestack#1001):
//! no credentials plus `Accept: application/json` is `406`, not `401`.

use cratestack::axum::body::Body;
use cratestack::axum::http::{Request, StatusCode};
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

/// `x-caller` authenticates; anything else is refused `401` by the provider
/// itself, so only a request that reaches `authenticate` can see a `401`.
#[derive(Clone)]
struct StrictCaller;

impl cratestack::AuthProvider for StrictCaller {
    type Error = CratestackError;

    fn authenticate(
        &self,
        request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        core::future::ready(if request.headers.contains_key("x-caller") {
            Ok(CratestackContext::authenticated([(
                "id".to_owned(),
                Value::Int(1),
            )]))
        } else {
            Err(CratestackError::Unauthorized("sign in".to_owned()))
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
        _args: countdown::Args,
        _authorized: countdown::Authorized,
    ) -> Result<SubscriptionStream<countdown::Output>, CratestackError> {
        Ok(Box::pin(cratestack::futures::stream::empty()))
    }
}

async fn status(accept: &str) -> StatusCode {
    let router = cratestack_schema::axum::router(
        cratestack_schema::Cratestack::builder().build(),
        Countdown,
        (),
        cratestack_codec_cbor::CborCodec,
        StrictCaller,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    );
    let request = Request::get("/$procs/countdown?args=%7B%22to%22%3A1%7D")
        .header("accept", accept)
        .body(Body::empty())
        .unwrap();
    router.oneshot(request).await.unwrap().status()
}

#[tokio::test]
async fn rest_procedure_refuses_a_bad_accept_with_406_before_authenticating() {
    assert_eq!(status("application/json").await, StatusCode::NOT_ACCEPTABLE);
    // The provider does refuse this caller: with a right `Accept` it is 401.
    assert_eq!(status("text/event-stream").await, StatusCode::UNAUTHORIZED);
}
