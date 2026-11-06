//! A `subscription procedure` (`docs/design/subscriptions-websocket.md`
//! §3.2, §3.3, slice S4) over SSE, RPC and REST, and through the
//! dispatcher, fed by a `topic` (§3.4): its policy and the handler's setup
//! refuse before `Ready`, a topic delivers under its key only, and
//! `publish_in_tx` delivers only what commits.
//!
//! Postgres-backed (the topic rides `PgNotifyBackplane`): run it with
//! `CRATESTACK_REQUIRE_DB=1`, or a missing database is a skip that still
//! prints `ok` (CLAUDE.md, "Critical test gotcha").

use std::time::Duration;

use cratestack::axum::body::Body;
use cratestack::axum::http::{Request, StatusCode};
use cratestack::axum::response::Response;
use cratestack::futures::StreamExt;
use cratestack::serde_json::json;
use cratestack::{
    AuthProvider, CratestackContext, CratestackError, RequestContext, SubscriptionDispatcher,
    SubscriptionOpen, SubscriptionStream, Value, include_server_schema,
};
use tokio::time::timeout;
use tower::util::ServiceExt;

include_server_schema!(
    "tests/fixtures/subscription_procedures.cstack",
    db = Postgres
);

mod support;

use cratestack_schema::procedures::room_messages;
use support::pg;

/// `x-caller: <id>` authenticates as that id; no header is anonymous.
#[derive(Clone)]
struct HeaderCaller;

impl AuthProvider for HeaderCaller {
    type Error = CratestackError;

    fn authenticate(
        &self,
        request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        let caller = request
            .headers
            .get("x-caller")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<i64>().ok());
        core::future::ready(Ok(match caller {
            Some(id) => CratestackContext::authenticated([("id".to_owned(), Value::Int(id))]),
            None => CratestackContext::anonymous(),
        }))
    }
}

/// Forwards a room's topic; refuses the room `secret` during setup.
#[derive(Clone)]
struct Rooms;

impl cratestack_schema::procedures::ProcedureRegistry for Rooms {
    async fn room_messages(
        &self,
        db: &cratestack_schema::Cratestack,
        _ctx: &CratestackContext,
        args: room_messages::Args,
        _authorized: room_messages::Authorized,
    ) -> Result<SubscriptionStream<room_messages::Output>, CratestackError> {
        if args.args.roomId == "secret" {
            return Err(CratestackError::Forbidden(
                "not a member of this room".to_owned(),
            ));
        }
        db.topics()
            .room_message(&args.args.roomId)
            .subscribe()
            .await
    }
}

fn message(room: &str, body: &str) -> cratestack_schema::ChatMessage {
    cratestack_schema::ChatMessage {
        roomId: room.to_owned(),
        body: body.to_owned(),
    }
}

async fn send(router: &cratestack::axum::Router, request: Request<Body>) -> Response {
    router
        .clone()
        .oneshot(request)
        .await
        .expect("subscribe route")
}

fn get(uri: &str, caller: Option<i64>) -> Request<Body> {
    let mut request = Request::get(uri).header("accept", "text/event-stream");
    if let Some(id) = caller {
        request = request.header("x-caller", id.to_string());
    }
    request.body(Body::empty()).unwrap()
}

/// The bodies of the messages the stream sends within `window`.
async fn bodies(response: Response, window: Duration) -> Vec<String> {
    let mut body = response.into_body().into_data_stream();
    let mut text = Vec::new();
    while let Ok(Some(chunk)) = timeout(window, body.next()).await {
        text.extend_from_slice(&chunk.expect("body chunk"));
    }
    String::from_utf8(text)
        .expect("SSE is UTF-8")
        .split("\n\n")
        .filter_map(|event| event.strip_prefix("event: message\ndata: "))
        .map(|data| {
            let frame: cratestack::serde_json::Value =
                cratestack::serde_json::from_str(data).expect("frame is JSON");
            frame["next"]["body"].as_str().expect("a body").to_owned()
        })
        .collect()
}

#[tokio::test]
async fn a_subscription_procedure_streams_its_topic() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let db = cratestack_schema::Cratestack::builder(test_pg.pool.clone()).build();
    let router = cratestack_schema::axum::rpc_router(
        db.clone(),
        Rooms,
        (),
        cratestack_codec_cbor::CborCodec,
        HeaderCaller,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    );
    let op = "/rpc/subscribe/procedure.roomMessages";
    let r1 = r#"args=%7B%22roomId%22%3A%22r1%22%7D"#;

    let sse = send(&router, get(&format!("{op}?{r1}"), Some(1))).await;
    assert_eq!(sse.status(), StatusCode::OK);
    // The same subscription as a JSON body, the input WebTransport carries.
    let posted = Request::post(op)
        .header("accept", "text/event-stream")
        .header("x-caller", "1")
        .body(Body::from(r#"{"args":{"roomId":"r1"}}"#))
        .unwrap();
    let posted = send(&router, posted).await;
    assert_eq!(posted.status(), StatusCode::OK);
    let mut dispatched = db
        .subscriptions(Rooms)
        .open(SubscriptionOpen {
            op_id: "procedure.roomMessages".to_owned(),
            input: json!({"args": {"roomId": "r1"}}),
            ctx: CratestackContext::authenticated([("id".to_owned(), Value::Int(1))]),
        })
        .await
        .expect("the dispatcher opens it");

    // Refusals before Ready: the procedure's `@allow`, then the handler's
    // setup, then arguments that are not the procedure's.
    let anonymous = send(&router, get(&format!("{op}?{r1}"), None)).await;
    assert_eq!(anonymous.status(), StatusCode::FORBIDDEN);
    let secret = r#"args=%7B%22roomId%22%3A%22secret%22%7D"#;
    let refused = send(&router, get(&format!("{op}?{secret}"), Some(1))).await;
    assert_eq!(refused.status(), StatusCode::FORBIDDEN);
    let bad = send(&router, get(&format!("{op}?args=r1"), Some(1))).await;
    assert_eq!(bad.status(), StatusCode::BAD_REQUEST);

    let topics = db.topics();
    topics
        .room_message("r1")
        .publish(&message("r1", "one"))
        .await
        .expect("published");
    topics
        .room_message("r2")
        .publish(&message("r2", "elsewhere"))
        .await
        .expect("published");
    // Delivered only if the transaction commits.
    db.transaction(async |tx| {
        topics
            .room_message("r1")
            .publish_in_tx(tx, &message("r1", "committed"))
            .await
    })
    .await
    .expect("committed");
    let rolled_back = db
        .transaction(async |tx| {
            topics
                .room_message("r1")
                .publish_in_tx(tx, &message("r1", "rolled back"))
                .await?;
            Err::<(), _>(CratestackError::Conflict("roll back".to_owned()))
        })
        .await;
    assert!(rolled_back.is_err());
    topics
        .room_message("r1")
        .publish(&message("r1", "two"))
        .await
        .expect("published");

    let window = Duration::from_millis(750);
    let (sse, posted) = tokio::join!(bodies(sse, window), bodies(posted, window));
    assert_eq!(sse, ["one", "committed", "two"]);
    assert_eq!(posted, sse);
    let first = dispatched
        .next()
        .await
        .expect("an item")
        .expect("not an error");
    assert_eq!(first["body"], "one");
}

/// The client contract's op list is what the macro emits (cratestack#1123):
/// a subscription procedure is `procedure.<name>`, `OpKind::Subscription`.
#[test]
fn the_contract_lists_the_subscription_op() {
    let schema =
        cratestack_parser::parse_schema_file("tests/fixtures/subscription_procedures.cstack")
            .expect("fixture parses");
    let keys = cratestack_core::client_contract::op_keys(&schema);
    let ops = cratestack_schema::axum::OPS;
    assert_eq!(keys, ["procedure.roomMessages"]);
    assert_eq!(
        ops.iter().map(|op| op.op_id).collect::<Vec<_>>(),
        ["procedure.roomMessages"]
    );
    assert!(matches!(ops[0].kind, cratestack::OpKind::Subscription));
}

mod rest {
    use super::*;

    #[test]
    fn the_contract_lists_the_rest_route() {
        let schema = cratestack_parser::parse_schema_file(
            "tests/fixtures/subscription_procedures_rest.cstack",
        )
        .expect("fixture parses");
        let routes: Vec<String> = cratestack_schema::axum::ROUTE_TRANSPORTS
            .iter()
            .map(|r| format!("{} {}", r.method, r.path))
            .collect();
        assert_eq!(routes, ["GET /$procs/roomMessages"]);
        assert_eq!(cratestack_core::client_contract::op_keys(&schema), routes);
    }

    include_server_schema!(
        "tests/fixtures/subscription_procedures_rest.cstack",
        db = Postgres
    );

    #[derive(Clone)]
    struct RestRooms;

    impl cratestack_schema::procedures::ProcedureRegistry for RestRooms {
        async fn room_messages(
            &self,
            db: &cratestack_schema::Cratestack,
            _ctx: &CratestackContext,
            args: cratestack_schema::procedures::room_messages::Args,
            _authorized: cratestack_schema::procedures::room_messages::Authorized,
        ) -> Result<
            SubscriptionStream<cratestack_schema::procedures::room_messages::Output>,
            CratestackError,
        > {
            db.topics()
                .room_message(&args.args.roomId)
                .subscribe()
                .await
        }
    }

    #[tokio::test]
    async fn rest_serves_it_at_the_procedure_path() {
        let Some(test_pg) = pg::connect_or_skip().await else {
            return;
        };
        let db = cratestack_schema::Cratestack::builder(test_pg.pool.clone()).build();
        let router = cratestack_schema::axum::router(
            db.clone(),
            RestRooms,
            (),
            cratestack_codec_cbor::CborCodec,
            HeaderCaller,
            cratestack::DEFAULT_BODY_LIMIT_BYTES,
        );
        let uri = r#"/$procs/roomMessages?args=%7B%22roomId%22%3A%22rest%22%7D"#;
        let sse = send(&router, get(uri, Some(1))).await;
        assert_eq!(sse.status(), StatusCode::OK);
        let anonymous = send(&router, get(uri, None)).await;
        assert_eq!(anonymous.status(), StatusCode::FORBIDDEN);
        db.topics()
            .room_message("rest")
            .publish(&cratestack_schema::ChatMessage {
                roomId: "rest".to_owned(),
                body: "over REST".to_owned(),
            })
            .await
            .expect("published");
        assert_eq!(bodies(sse, Duration::from_millis(750)).await, ["over REST"]);
    }
}
