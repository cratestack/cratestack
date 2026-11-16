//! A subscribe request with a wrong `Accept` is refused `406` *before* it is
//! authenticated (cratestack#1001): a caller with no credentials who also
//! asks for the wrong media type learns about the media type, not `401`, and
//! no credential work happens for a request that cannot be served. Both RPC
//! subscribe routes, `GET` and `POST /rpc/subscribe/{op_id}`.

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

use cratestack_schema::procedures::{countdown, watch_room};

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
struct Procedures;

impl cratestack_schema::procedures::ProcedureRegistry for Procedures {
    async fn countdown(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        _args: countdown::Args,
        _authorized: countdown::Authorized,
    ) -> Result<SubscriptionStream<countdown::Output>, CratestackError> {
        Ok(Box::pin(cratestack::futures::stream::empty()))
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

fn router() -> cratestack::axum::Router {
    cratestack_schema::axum::rpc_router(
        cratestack_schema::Cratestack::builder().build(),
        Procedures,
        (),
        cratestack_codec_cbor::CborCodec,
        StrictCaller,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
}

async fn status(request: Request<Body>) -> StatusCode {
    router().oneshot(request).await.unwrap().status()
}

fn get(accept: &str) -> Request<Body> {
    Request::get("/rpc/subscribe/procedure.countdown?args=%7B%22to%22%3A1%7D")
        .header("accept", accept)
        .body(Body::empty())
        .unwrap()
}

fn post(accept: &str) -> Request<Body> {
    Request::post("/rpc/subscribe/procedure.countdown")
        .header("accept", accept)
        .header("content-type", "application/json")
        .body(Body::from(r#"{"to":1}"#))
        .unwrap()
}

#[tokio::test]
async fn rpc_get_refuses_a_bad_accept_with_406_before_authenticating() {
    assert_eq!(
        status(get("application/json")).await,
        StatusCode::NOT_ACCEPTABLE
    );
    // The provider does refuse this caller: with a right `Accept` it is 401.
    assert_eq!(
        status(get("text/event-stream")).await,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn rpc_post_refuses_a_bad_accept_with_406_before_authenticating() {
    assert_eq!(
        status(post("application/json")).await,
        StatusCode::NOT_ACCEPTABLE
    );
    assert_eq!(
        status(post("text/event-stream")).await,
        StatusCode::UNAUTHORIZED
    );
}
