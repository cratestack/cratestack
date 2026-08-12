//! End-to-end proof of cratestack#390's core claim: a real
//! `GET /rpc/subscribe/{op_id}` SSE connection actually receives
//! `ModelEvent<Widget>` items as the underlying model changes — not
//! just that the schema parses or the route exists — plus the
//! backpressure-overflow termination path (design doc §3.4/§3.4a).
//!
//! `subscribe_sse_receives_model_events_as_they_happen` drives the REAL
//! generated `rpc_router` over a genuine TCP connection via `reqwest`
//! (not `tower::ServiceExt::oneshot`, which can't observe a real
//! streaming response) — the same pattern `examples/rpc-streaming/
//! tests/stream_wire_timing.rs` uses for `@stream` procedures. `reqwest`
//! (rather than a hand-rolled `TcpStream` reader) is what correctly
//! decodes `Transfer-Encoding: chunked` — axum streams this response
//! without a known `Content-Length`, so the wire bytes are
//! chunk-framed, not raw SSE text.
//!
//! `subscribe_sse_emits_unavailable_error_and_ends_stream_on_overflow`
//! drives the same router via `oneshot` instead, deliberately: filling
//! the bounded channel past capacity *before* the response body is ever
//! polled makes the overflow deterministic (no reliance on OS socket
//! buffer timing), while still exercising the real dispatch + encoding
//! path end to end.

use std::time::Duration;

use cratestack::axum::body::Body;
use cratestack::axum::http::{Request, StatusCode};
use cratestack::futures::StreamExt;
use cratestack::include_server_schema;
use cratestack::{AuthProvider, CoolContext, CoolError, RequestContext, Value};
use cratestack_codec_cbor::CborCodec;
use tokio::net::TcpListener;
use tokio::time::timeout;
use tower::util::ServiceExt;

include_server_schema!("tests/fixtures/transport_rpc.cstack", db = Postgres);

mod support;

use support::pg;

const READ_TIMEOUT: Duration = Duration::from_secs(5);

/// Always authenticates as principal `1` — subscribe dispatch only
/// needs a successful `AuthProvider::authenticate`, the same header-
/// based contract every other HTTP RPC binding uses (§3.4a).
#[derive(Clone)]
struct AlwaysAuthProvider;

impl AuthProvider for AlwaysAuthProvider {
    type Error = CoolError;

    fn authenticate(
        &self,
        _request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CoolContext, Self::Error>> + Send {
        core::future::ready(Ok(CoolContext::authenticated([(
            "id".to_owned(),
            Value::Int(1),
        )])))
    }
}

#[derive(Clone)]
struct RpcProcedures;

impl cratestack_schema::procedures::ProcedureRegistry for RpcProcedures {
    fn ping(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CoolContext,
        args: cratestack_schema::procedures::ping::Args,
        _authorized: cratestack_schema::procedures::ping::Authorized,
    ) -> impl core::future::Future<
        Output = Result<cratestack_schema::procedures::ping::Output, CoolError>,
    > + Send {
        core::future::ready(Ok(args.args))
    }

    fn bump(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CoolContext,
        args: cratestack_schema::procedures::bump::Args,
        _authorized: cratestack_schema::procedures::bump::Authorized,
    ) -> impl core::future::Future<
        Output = Result<cratestack_schema::procedures::bump::Output, CoolError>,
    > + Send {
        core::future::ready(Ok(args.args))
    }

    fn many_pings(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CoolContext,
        args: cratestack_schema::procedures::many_pings::Args,
        _authorized: cratestack_schema::procedures::many_pings::Authorized,
    ) -> impl core::future::Future<
        Output = Result<cratestack_schema::procedures::many_pings::Output, CoolError>,
    > + Send {
        core::future::ready(Ok(vec![args.args]))
    }
}

async fn reset_widgets_table(pool: &cratestack::sqlx::PgPool) {
    cratestack::sqlx::query("DROP TABLE IF EXISTS widgets")
        .execute(pool)
        .await
        .expect("drop widgets");
    cratestack::sqlx::query("CREATE TABLE widgets (id BIGINT PRIMARY KEY, name TEXT NOT NULL)")
        .execute(pool)
        .await
        .expect("create widgets");
    // The outbox is shared per-database; a stale row from an earlier
    // test run in this same external database would let `drain()` pick
    // up more than the one event this test just caused.
    cratestack::sqlx::query("DROP TABLE IF EXISTS cratestack_event_outbox")
        .execute(pool)
        .await
        .expect("reset event outbox");
}

/// The real end-to-end proof: subscribe over SSE, mutate the model
/// through the normal write path (which drains the outbox itself on
/// commit — the same mechanism `@@emit` has always used), and assert
/// the SSE stream receives the corresponding event.
#[tokio::test]
async fn subscribe_sse_receives_model_events_as_they_happen() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_widgets_table(pool).await;

    let db = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = cratestack_schema::axum::rpc_router(
        db.clone(),
        RpcProcedures,
        CborCodec,
        AlwaysAuthProvider,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    );
    tokio::spawn(async move {
        cratestack::axum::serve(listener, app).await.unwrap();
    });

    // #440: `reqwest`'s `rustls-no-provider` feature needs a crypto provider
    // installed before `Client::new()` — everywhere else in this workspace
    // that builds a client this way gets it via `CratestackClient::new`'s
    // own fallback (`cratestack-client-rust`'s `ensure_crypto_provider`);
    // this test talks to the SSE endpoint with a bare `reqwest::Client`
    // instead (see the comment on `reqwest.workspace = true` in this
    // crate's `Cargo.toml` dev-dependencies), so it needs its own call.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = reqwest::Client::new();
    let mut response = timeout(
        READ_TIMEOUT,
        client
            .get(format!(
                "http://{addr}/rpc/subscribe/model.Widget.subscribe"
            ))
            .header("accept", "text/event-stream")
            .send(),
    )
    .await
    .expect("connect should not time out")
    .expect("subscribe request should succeed");

    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("text/event-stream"),
    );

    // Mutate the subscribed model through the real write path.
    // `CreateRecord::run` already drains the outbox itself once its
    // transaction commits (`crates/cratestack-sqlx/src/query/write/
    // create.rs`) — the same mechanism `@@emit` has always used to hand
    // events to `CoolEventBus`
    // (`crates/cratestack-sqlx/src/descriptor.rs::drain_event_outbox`) —
    // so by the time `.run()` returns, delivery has already happened;
    // no separate manual drain step is needed here.
    let ctx = CoolContext::authenticated([("id".to_owned(), Value::Int(1))]);
    db.widget()
        .bind(ctx)
        .create(cratestack_schema::CreateWidgetInput {
            id: 1,
            name: "Gadget".to_owned(),
        })
        .run()
        .await
        .expect("widget should be created");

    // Read the SSE event actually delivered over the wire, chunk by
    // chunk, until a complete `event: ...\ndata: ...\n\n` block has
    // arrived.
    let mut body = Vec::new();
    loop {
        if find_subslice(&body, b"\n\n").is_some() {
            break;
        }
        let chunk = timeout(READ_TIMEOUT, response.chunk())
            .await
            .expect("body chunk should arrive before the timeout")
            .expect("body chunk read should succeed")
            .expect("stream should not end before the event arrives");
        body.extend_from_slice(&chunk);
    }
    let event_text = String::from_utf8(body).expect("SSE body should be valid UTF-8");
    assert!(
        event_text.starts_with("event: message\n"),
        "expected a message event, got: {event_text}"
    );
    assert!(
        event_text.contains("\"model\":\"Widget\""),
        "event payload should carry the model name: {event_text}"
    );
    assert!(
        event_text.contains("\"operation\":\"Created\""),
        "event payload should carry the operation: {event_text}"
    );
    assert!(
        event_text.contains("\"name\":\"Gadget\""),
        "event payload should carry the actual row data: {event_text}"
    );
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Deterministic proof of the backpressure-overflow path: fill the
/// bounded channel past capacity *before* the SSE response body is ever
/// polled (so the overflow doesn't depend on OS socket buffer timing),
/// then assert the terminal bytes are exactly one `Error{unavailable}`
/// SSE event, and that the body then ends — not a hang, not a silent
/// drop.
#[tokio::test]
async fn subscribe_sse_emits_unavailable_error_and_ends_stream_on_overflow() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let pool = &test_pg.pool;
    reset_widgets_table(pool).await;

    let db = cratestack_schema::Cratestack::builder(pool.clone()).build();
    let router = cratestack_schema::axum::rpc_router(
        db.clone(),
        RpcProcedures,
        CborCodec,
        AlwaysAuthProvider,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    );

    let response = router
        .oneshot(
            Request::get("/rpc/subscribe/model.Widget.subscribe")
                .header("accept", "text/event-stream")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("subscribe dispatch should succeed");
    assert_eq!(response.status(), StatusCode::OK);

    // Past this point the subscription is live (the handler is
    // registered on `CoolEventBus`) but nothing has polled the response
    // body yet, so every `push()` below runs synchronously in this
    // task, uncontested — the 65th create is guaranteed to observe a
    // genuinely full channel, not a race against a consumer.
    // `CreateRecord::run` drains the outbox itself on commit (see the
    // sibling test's comment), so each iteration below both enqueues
    // *and* delivers its own event synchronously — no separate manual
    // drain step is needed.
    let ctx = CoolContext::authenticated([("id".to_owned(), Value::Int(1))]);
    const OVERFLOW_COUNT: i64 = 65; // one past SUBSCRIPTION_BUFFER_CAPACITY (64)
    for id in 0..OVERFLOW_COUNT {
        db.widget()
            .bind(ctx.clone())
            .create(cratestack_schema::CreateWidgetInput {
                id,
                name: format!("Gadget {id}"),
            })
            .run()
            .await
            .expect("widget should be created");
    }

    // Drain the response body: some prefix of `message` events (however
    // many made it into the channel before it filled), then exactly one
    // `error` event, then the stream ends.
    let mut data_stream = response.into_body().into_data_stream();
    let mut collected = Vec::new();
    while let Some(chunk) = timeout(READ_TIMEOUT, data_stream.next())
        .await
        .expect("body stream should not hang")
    {
        collected.extend_from_slice(&chunk.expect("body chunk should decode"));
    }
    let body_text = String::from_utf8(collected).expect("SSE body should be valid UTF-8");

    let events: Vec<&str> = body_text.split("\n\n").filter(|s| !s.is_empty()).collect();
    assert!(
        !events.is_empty(),
        "expected at least the terminal error event"
    );
    let (message_events, error_events): (Vec<&&str>, Vec<&&str>) = events
        .iter()
        .partition(|event| event.starts_with("event: message"));
    assert_eq!(
        error_events.len(),
        1,
        "expected exactly one terminal error event, got: {body_text}"
    );
    assert!(
        error_events[0].contains("\"code\":\"unavailable\""),
        "terminal event should carry the unavailable code: {}",
        error_events[0]
    );
    assert!(
        error_events[0].contains("\"message\":\"subscription lagged\""),
        "terminal event should carry the lagged message: {}",
        error_events[0]
    );
    assert!(
        message_events.len() <= 64,
        "no more than the channel capacity's worth of message events should have gotten through, \
         got {}",
        message_events.len(),
    );
    // The error event must be the LAST thing in the body — the encoder
    // never resumes normal output after it (mirrors §3.3's cbor-seq
    // tag-48900 sentinel contract, applied to SSE).
    assert!(
        events.last().unwrap().starts_with("event: error"),
        "error event must be the final event in the stream: {body_text}"
    );
}
