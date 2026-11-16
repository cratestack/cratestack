//! The `application/cbor-seq` subscription binding on a REST subscription
//! procedure (cratestack#1001): `GET /$procs/<name>` answers a CBOR sequence
//! of `ServerFrame`s when `Accept` asks for it. REST has no `POST` subscribe
//! form, SSE or not, so there is none to test.

use cratestack::axum::body::{Body, to_bytes};
use cratestack::axum::http::{Request, StatusCode, header};
use cratestack::{CratestackContext, CratestackError, SubscriptionStream, include_server_schema};
use cratestack_codec_cbor::CborCodec;
use tower::util::ServiceExt;

include_server_schema!(
    "tests/fixtures/subscription_cbor_seq_rest.cstack",
    db = None,
    decimal = RustDecimal
);

mod cbor_seq_support;

use cbor_seq_support::Caller;

use cratestack_schema::procedures::readings;

#[derive(Clone)]
struct Readings;

impl cratestack_schema::procedures::ProcedureRegistry for Readings {
    async fn readings(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        args: readings::Args,
        _authorized: readings::Authorized,
    ) -> Result<SubscriptionStream<readings::Output>, CratestackError> {
        let count = args.args.count;
        // `0`: quiet. Negative: a terminal error. Otherwise `count` readings.
        if count == 0 {
            return Ok(Box::pin(cratestack::futures::stream::pending()));
        }
        let items: Vec<Result<cratestack_schema::Reading, CratestackError>> = if count < 0 {
            vec![Err(CratestackError::Unavailable("lagged".to_owned()))]
        } else {
            (1..=count)
                .map(|n| {
                    Ok(cratestack_schema::Reading {
                        id: cbor_seq_support::expected_id(n),
                        data: vec![0, 1, 255, n as u8],
                        amount: format!("{n}.50").parse().unwrap(),
                        n,
                    })
                })
                .collect()
        };
        Ok(Box::pin(cratestack::futures::stream::iter(items)))
    }
}

fn router() -> cratestack::axum::Router {
    cratestack_schema::axum::router(
        cratestack_schema::Cratestack::builder().build(),
        Readings,
        (),
        CborCodec,
        Caller,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
}

fn get(accept: Option<&str>, count: i64) -> Request<Body> {
    let mut request = Request::get(format!("/$procs/readings?args=%7B%22count%22%3A{count}%7D"))
        .header("x-caller", "1");
    if let Some(accept) = accept {
        request = request.header("accept", accept);
    }
    request.body(Body::empty()).unwrap()
}

async fn serve(request: Request<Body>) -> (StatusCode, Option<String>, Vec<u8>) {
    let response = router().oneshot(request).await.unwrap();
    let status = response.status();
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .map(|value| value.to_str().unwrap().to_owned());
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, content_type, bytes.to_vec())
}

#[tokio::test]
async fn get_sends_ready_first_then_lossless_items_then_end() {
    let (status, content_type, bytes) = serve(get(Some("application/cbor-seq"), 2)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type.as_deref(), Some("application/cbor-seq"));
    let body = cbor_seq_support::read(&bytes);
    assert_eq!(body.keepalives, 0);
    assert_eq!(body.frames.len(), 4, "ready, two readings, end");
    assert_eq!(body.frames[0].id, 1);
    assert!(body.frames[0].ready.is_some());
    cbor_seq_support::assert_reading(&body.frames[1], 2, 1);
    cbor_seq_support::assert_reading(&body.frames[2], 3, 2);
    assert_eq!(body.frames[3].id, 4);
    assert!(body.frames[3].end.is_some(), "a source that ran out ends");
}

#[tokio::test]
async fn a_terminal_error_is_an_err_frame_and_the_last() {
    let (status, _, bytes) = serve(get(Some("application/cbor-seq"), -1)).await;
    assert_eq!(status, StatusCode::OK);
    let body = cbor_seq_support::read(&bytes);
    assert_eq!(body.frames.len(), 2, "ready, then the error");
    let err = body.frames[1].err.as_ref().expect("an err frame");
    assert_eq!(
        (err.code.as_str(), err.message.as_str()),
        ("unavailable", "lagged")
    );
}

#[tokio::test]
async fn sse_is_unchanged_and_a_bad_accept_is_406() {
    for accept in [Some("text/event-stream"), Some("*/*")] {
        let (status, content_type, _) = serve(get(accept, 1)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(content_type.as_deref(), Some("text/event-stream"));
    }
    for accept in [None, Some("application/json"), Some("application/cbor")] {
        let (status, _, _) = serve(get(accept, 1)).await;
        assert_eq!(status, StatusCode::NOT_ACCEPTABLE, "{accept:?}");
    }
}

#[tokio::test]
async fn an_unauthenticated_caller_is_refused_before_any_frame() {
    let request = Request::get("/$procs/readings?args=%7B%22count%22%3A1%7D")
        .header("accept", "application/cbor-seq")
        .body(Body::empty())
        .unwrap();
    let (status, content_type, _) = serve(request).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(content_type.as_deref(), Some("application/json"));
}

/// A quiet subscription sends a bare CBOR `null` every 15 s, after `ready`.
#[tokio::test(start_paused = true)]
async fn a_quiet_subscription_sends_a_null_keepalive() {
    use cratestack::futures::StreamExt;

    let response = router()
        .oneshot(get(Some("application/cbor-seq"), 0))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut body = response.into_body().into_data_stream();
    let ready = body.next().await.unwrap().unwrap();
    assert!(cbor_seq_support::read(&ready).frames[0].ready.is_some());
    assert_eq!(&body.next().await.unwrap().unwrap()[..], &[0xf6]);
    assert_eq!(&body.next().await.unwrap().unwrap()[..], &[0xf6]);
}
