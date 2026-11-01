//! The chat over each binding the generated Rust client speaks (slice S6):
//! messages as a model feed over the WebSocket, typing indicators from the
//! topic over SSE, a unary `sendTyping` publishing them, and `Auto` falling
//! back to SSE where no socket opens (a proxy that does not pass upgrades).
//! WebTransport and the browser's own fallback, with UDP blocked, are
//! `tests/browser/fallback.cjs` (`just test-realtime-chat`).
//!
//! Postgres-backed: `CRATESTACK_TEST_DATABASE_URL`, or a skip that still
//! prints `ok` unless `CRATESTACK_REQUIRE_DB` is set.

use std::sync::Arc;
use std::time::Duration;

use cratestack::client_rust::{
    AuthorizationRequest, ClientConfig, ClientError, CratestackClient, RequestAuthorizer,
    SubscriptionTransport,
};
use cratestack::sqlx::PgPool;
use cratestack::{CratestackContext, JsonCodec, Value};
use realtime_chat_example::cratestack_schema::{self, CreateMessageInput, Room, Typing};
use realtime_chat_example::{Chat, Cratestack, SCHEMA_SQL};

async fn database() -> Option<Cratestack> {
    let Ok(url) = std::env::var("CRATESTACK_TEST_DATABASE_URL") else {
        assert!(
            std::env::var_os("CRATESTACK_REQUIRE_DB").is_none(),
            "CRATESTACK_REQUIRE_DB is set but CRATESTACK_TEST_DATABASE_URL is not"
        );
        eprintln!("skipping: CRATESTACK_TEST_DATABASE_URL is not set");
        return None;
    };
    let pool = PgPool::connect(&url).await.expect("the test database");
    for statement in ["DROP TABLE IF EXISTS messages", SCHEMA_SQL] {
        cratestack::sqlx::query(statement)
            .execute(&pool)
            .await
            .expect(statement);
    }
    Some(Cratestack::builder(pool).build())
}

/// User 1, on every request the client sends (a socket's upgrade included).
struct AsUserOne;

#[async_trait::async_trait]
impl RequestAuthorizer for AsUserOne {
    async fn authorize(
        &self,
        _request: &AuthorizationRequest,
    ) -> Result<Vec<(String, String)>, ClientError> {
        Ok(vec![("authorization".to_owned(), "Bearer 1".to_owned())])
    }
}

async fn serve(app: cratestack::axum::Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/", listener.local_addr().unwrap());
    tokio::spawn(async move { cratestack::axum::serve(listener, app).await.unwrap() });
    base
}

fn client(
    base: &str,
    transport: SubscriptionTransport,
) -> cratestack_schema::client::Client<JsonCodec> {
    let config = ClientConfig::new(base.parse().unwrap()).with_subscription_transport(transport);
    cratestack_schema::client::Client::new(
        CratestackClient::new(config, JsonCodec).with_request_authorizer(Arc::new(AsUserOne)),
    )
}

async fn next<T>(subscription: &mut cratestack::client_rust::ClientSubscription<T>) -> T {
    tokio::time::timeout(Duration::from_secs(5), subscription.recv())
        .await
        .expect("an item in time")
        .expect("still open")
        .expect("an item, not an error")
}

fn room(id: &str) -> cratestack_schema::procedures::typing::Args {
    cratestack_schema::procedures::typing::Args {
        args: Room {
            roomId: id.to_owned(),
        },
    }
}

#[tokio::test]
async fn messages_over_the_socket_and_typing_over_sse() {
    let Some(db) = database().await else { return };
    let base = serve(realtime_chat_example::router(db.clone())).await;

    let mut messages = client(&base, SubscriptionTransport::WebSocket)
        .messages()
        .subscribe(&Default::default())
        .await
        .expect("the feed opens over the socket");
    let mut typing = client(&base, SubscriptionTransport::Sse)
        .procedures()
        .typing(&room("lobby"))
        .await
        .expect("the procedure opens over SSE");

    db.message()
        .bind(CratestackContext::authenticated([(
            "id".to_owned(),
            Value::Int(1),
        )]))
        .create(CreateMessageInput {
            id: 1,
            roomId: "lobby".to_owned(),
            authorId: 1,
            body: "hello".to_owned(),
        })
        .run()
        .await
        .expect("message posted");
    let event = next(&mut messages).await;
    assert_eq!((event.data.id, event.data.body.as_str()), (1, "hello"));

    // A unary call publishes to the topic; the subscription sees it.
    let sent = client(&base, SubscriptionTransport::Sse)
        .procedures()
        .send_typing(&cratestack_schema::procedures::send_typing::Args {
            args: Typing {
                roomId: "lobby".to_owned(),
                who: "ada".to_owned(),
            },
        })
        .await
        .expect("sendTyping");
    assert_eq!(sent.who, "ada");
    assert_eq!(next(&mut typing).await.who, "ada");
}

#[tokio::test]
async fn auto_falls_back_to_sse_where_no_socket_opens() {
    let Some(db) = database().await else { return };
    // The unary and SSE routes only: what a proxy that drops upgrades leaves.
    let base = serve(cratestack_schema::axum::rpc_router(
        db.clone(),
        Chat,
        (),
        JsonCodec,
        realtime_chat_example::BearerId,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    ))
    .await;

    let mut typing = client(&base, SubscriptionTransport::Auto)
        .procedures()
        .typing(&room("r2"))
        .await
        .expect("auto opens over SSE");
    db.topics()
        .room_typing("r2")
        .publish(&Typing {
            roomId: "r2".to_owned(),
            who: "grace".to_owned(),
        })
        .await
        .expect("published");
    assert_eq!(next(&mut typing).await.who, "grace");
}
