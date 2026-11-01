//! The generated Rust client over WebTransport (`client-webtransport`)
//! against this example's real QUIC listener and Postgres:
//!
//! - pinned to WebTransport, with the HTTP base URL on a dead port, so no
//!   other binding could have delivered;
//! - `Auto` taking WebTransport first;
//! - `Auto` with the WebTransport URL on a dead UDP port: it falls back to
//!   the WebSocket, and remembers, so the next subscription does not wait
//!   again.
//!
//! Postgres-backed: `CRATESTACK_TEST_DATABASE_URL`, or a skip that still
//! prints `ok` unless `CRATESTACK_REQUIRE_DB` is set.

use std::sync::Arc;
use std::time::{Duration, Instant};

use cratestack::JsonCodec;
use cratestack::client_rust::{
    AuthorizationRequest, ClientConfig, ClientError, CratestackClient, RequestAuthorizer,
    ServerTrust, SubscriptionTransport,
};
use cratestack::sqlx::PgPool;
use cratestack::webtransport::DevCertificate;
use realtime_chat_example::cratestack_schema::{self, Room, Typing};
use realtime_chat_example::{Cratestack, SCHEMA_SQL};
use url::Url;

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

/// The QUIC listener, and the trust that pins its dev certificate.
fn webtransport(db: &Cratestack) -> (Url, ServerTrust) {
    let certificate = DevCertificate::generate(&["localhost", "127.0.0.1"]).unwrap();
    let server = realtime_chat_example::webtransport(
        db,
        "127.0.0.1:0".parse().unwrap(),
        certificate.certificate_chain,
        certificate.private_key,
        "http://unused.invalid",
    )
    .unwrap();
    let url = format!(
        "https://127.0.0.1:{}/rpc/wt",
        server.local_addr().unwrap().port()
    );
    tokio::spawn(server.serve());
    (
        url.parse().unwrap(),
        ServerTrust::CertificateHashes(vec![certificate.sha256]),
    )
}

/// A local port nothing listens on, TCP or UDP.
fn dead_port() -> u16 {
    std::net::UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn client(config: ClientConfig) -> cratestack_schema::client::Client<JsonCodec> {
    cratestack_schema::client::Client::new(
        CratestackClient::new(config, JsonCodec).with_request_authorizer(Arc::new(AsUserOne)),
    )
}

fn room(id: &str) -> cratestack_schema::procedures::typing::Args {
    cratestack_schema::procedures::typing::Args {
        args: Room {
            roomId: id.to_owned(),
        },
    }
}

async fn publish_then_receive(
    db: &Cratestack,
    room_id: &str,
    typing: &mut cratestack::client_rust::ClientSubscription<Typing>,
) -> String {
    db.topics()
        .room_typing(room_id)
        .publish(&Typing {
            roomId: room_id.to_owned(),
            who: "ada".to_owned(),
        })
        .await
        .expect("published");
    tokio::time::timeout(Duration::from_secs(5), typing.recv())
        .await
        .expect("an item in time")
        .expect("still open")
        .expect("an item")
        .who
}

#[tokio::test]
async fn pinned_and_auto_subscribe_over_webtransport() {
    let Some(db) = database().await else { return };
    let (url, trust) = webtransport(&db);
    // No HTTP server at all: only WebTransport can deliver.
    let nowhere: Url = format!("http://127.0.0.1:{}/", dead_port())
        .parse()
        .unwrap();

    for transport in [
        SubscriptionTransport::WebTransport,
        SubscriptionTransport::Auto,
    ] {
        let config = ClientConfig::new(nowhere.clone())
            .with_subscription_transport(transport)
            .with_webtransport(url.clone(), trust.clone());
        let mut typing = client(config)
            .procedures()
            .typing(&room("wt"))
            .await
            .unwrap_or_else(|error| panic!("{transport:?} opens over WebTransport: {error:?}"));
        assert_eq!(publish_then_receive(&db, "wt", &mut typing).await, "ada");
    }
}

#[tokio::test]
async fn auto_falls_back_to_the_socket_when_udp_is_blocked_and_remembers() {
    let Some(db) = database().await else { return };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base: Url = format!("http://{}/", listener.local_addr().unwrap())
        .parse()
        .unwrap();
    let app = realtime_chat_example::router(db.clone());
    tokio::spawn(async move { cratestack::axum::serve(listener, app).await.unwrap() });
    let (_, trust) = webtransport(&db);
    let blocked: Url = format!("https://127.0.0.1:{}/rpc/wt", dead_port())
        .parse()
        .unwrap();
    let client = client(ClientConfig::new(base).with_webtransport(blocked, trust));

    let started = Instant::now();
    let mut first = client
        .procedures()
        .typing(&room("udp"))
        .await
        .expect("falls back");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "bounded by Auto's wait"
    );
    assert_eq!(publish_then_receive(&db, "udp", &mut first).await, "ada");

    let started = Instant::now();
    let mut second = client
        .procedures()
        .typing(&room("udp2"))
        .await
        .expect("falls back");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "the failure is remembered: {:?}",
        started.elapsed()
    );
    assert_eq!(publish_then_receive(&db, "udp2", &mut second).await, "ada");
}
