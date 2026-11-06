//! The Rust client's compact wire keys against the real generated RPC
//! server: the generated `Client::with_compact_keys` negotiates compact,
//! reads compact responses (unary and `@stream` items), and only then sends
//! compact request bodies.

mod compact_support;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use compact_support::Anonymous;
use cratestack::axum::extract::Request;
use cratestack::axum::middleware::{Next, from_fn};
use cratestack::axum::response::Response;
use cratestack::compact::{COMPACT_CBOR, COMPACT_CBOR_SEQ};
use cratestack::{CodecSet, CratestackContext, CratestackError};
use cratestack_client_rust::{ClientConfig, CratestackClient};
use cratestack_codec_cbor::CborCodec;
use cratestack_codec_json::JsonCodec;

mod srv {
    cratestack::include_server_schema!("tests/fixtures/compact_rpc.cstack", db = None);
}
mod cli {
    cratestack::include_client_schema!("tests/fixtures/compact_rpc.cstack");
}

use cli::cratestack_schema as client_schema;
use srv::cratestack_schema as schema;

compact_procedures!(srv);

/// `(request Content-Type, response Content-Type)` of every call served.
type Log = Arc<Mutex<Vec<(String, String)>>>;

async fn spawn() -> (std::net::SocketAddr, Log, Arc<AtomicUsize>) {
    let executions = Arc::new(AtomicUsize::new(0));
    let executed = executions.clone();
    let log = Log::default();
    let seen = log.clone();
    let router = schema::axum::rpc_router(
        schema::Cratestack::builder().build(),
        Procedures,
        (),
        CodecSet::new(CborCodec, JsonCodec),
        Anonymous,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
    .layer(from_fn(move |request: Request, next: Next| {
        let executed = executed.clone();
        async move {
            executed.fetch_add(1, Ordering::SeqCst);
            next.run(request).await
        }
    }))
    .layer(
        cratestack_axum::idempotency::IdempotencyLayer::new(
            Arc::new(compact_support::idempotency::InMemoryIdempotencyStore::default()),
            std::time::Duration::from_secs(3600),
        )
        .with_principal_fingerprint(|_| "compact-test-principal".to_owned()),
    )
    .layer(from_fn(move |request: Request, next: Next| {
        let seen = seen.clone();
        async move {
            let sent = header(request.headers(), "content-type");
            let response: Response = next.run(request).await;
            let got = header(response.headers(), "content-type");
            seen.lock().unwrap().push((sent, got));
            response
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { cratestack::axum::serve(listener, router).await.unwrap() });
    (addr, log, executions)
}

fn header(headers: &cratestack::axum::http::HeaderMap, name: &str) -> String {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned()
}

fn client(addr: std::net::SocketAddr, compact: bool) -> client_schema::client::Client {
    let base = reqwest::Url::parse(&format!("http://{addr}")).unwrap();
    let runtime = CratestackClient::cbor(ClientConfig::new(base));
    if compact {
        client_schema::client::Client::with_compact_keys(runtime)
    } else {
        client_schema::client::Client::new(runtime)
    }
}

fn order() -> client_schema::Order {
    client_schema::Order {
        id: 7,
        name: "ada".to_owned(),
        note: None,
        address: client_schema::Address {
            street: "main".to_owned(),
            city: "oslo".to_owned(),
        },
        tags: vec!["a".to_owned(), "b".to_owned()],
    }
}

fn echo(order: client_schema::Order) -> client_schema::procedures::echo_order::Args {
    client_schema::procedures::echo_order::Args { order }
}

#[tokio::test]
async fn compact_is_negotiated_then_requests_follow() {
    let (addr, log, _) = spawn().await;
    let client = client(addr, true);
    for _ in 0..2 {
        let back = client
            .procedures()
            .echo_order(&echo(order()))
            .await
            .unwrap();
        assert_eq!(back.address.city, "oslo");
        assert_eq!(back.tags, ["a", "b"]);
    }
    let log = log.lock().unwrap();
    assert_eq!(
        log[0],
        ("application/cbor".to_owned(), COMPACT_CBOR.to_owned())
    );
    assert_eq!(log[1], (COMPACT_CBOR.to_owned(), COMPACT_CBOR.to_owned()));
}

#[tokio::test]
async fn off_is_the_plain_wire() {
    let (addr, log, _) = spawn().await;
    let client = client(addr, false);
    client
        .procedures()
        .echo_order(&echo(order()))
        .await
        .unwrap();
    let log = log.lock().unwrap();
    assert_eq!(
        log[0],
        ("application/cbor".to_owned(), "application/cbor".to_owned())
    );
}

#[tokio::test]
async fn stream_items_arrive_decoded() {
    let (addr, log, _) = spawn().await;
    let client = client(addr, true);
    let mut ticks = client
        .procedures()
        .ticks(&client_schema::procedures::ticks::Args { count: 3 })
        .await
        .unwrap();
    let mut labels = Vec::new();
    while let Some(item) = ticks.recv().await {
        labels.push(item.unwrap().label);
    }
    assert_eq!(labels, ["tick 0", "tick 1", "tick 2"]);
    assert_eq!(log.lock().unwrap()[0].1, COMPACT_CBOR_SEQ);
}

#[tokio::test]
async fn an_idempotent_retry_survives_1024_distinct_keys() {
    let (addr, log, executions) = spawn().await;
    let runtime = CratestackClient::cbor(ClientConfig::new(
        reqwest::Url::parse(&format!("http://{addr}")).unwrap(),
    ))
    .with_compact_key_bytes(schema::KEY_TABLES, schema::OP_CONTRACTS)
    .unwrap()
    .at(cratestack_client_rust::RouteRef::rpc("procedure.echoOrder"));
    for number in 0..1024 {
        let key = format!("key-{number}");
        let result: client_schema::Order = runtime
            .post(
                "/rpc/procedure.echoOrder",
                &echo(order()),
                &[("Idempotency-Key", key.as_str())],
            )
            .await
            .unwrap();
        assert_eq!(result.id, 7);
    }
    let retry: client_schema::Order = runtime
        .post(
            "/rpc/procedure.echoOrder",
            &echo(order()),
            &[("Idempotency-Key", "key-0")],
        )
        .await
        .expect("same-key same-input retry must replay, not conflict after compact cache fills");
    assert_eq!(retry.id, 7);
    assert_eq!(
        executions.load(Ordering::SeqCst),
        1024,
        "retry must not execute twice"
    );
    {
        let seen = log.lock().unwrap();
        assert_eq!(
            seen[0], seen[1024],
            "retry must retain its original wire types"
        );
    }
    let mut changed = order();
    changed.id = 8;
    let conflict = runtime
        .post::<_, client_schema::Order>(
            "/rpc/procedure.echoOrder",
            &echo(changed),
            &[("Idempotency-Key", "key-0")],
        )
        .await
        .unwrap_err();
    assert!(conflict.to_string().contains("idempotency_key_conflict"));
    assert_eq!(
        executions.load(Ordering::SeqCst),
        1024,
        "conflict must not execute"
    );
}

prepared_replay_suite!("procedure.echoOrder", "/rpc/procedure.echoOrder");
prepared_auth_suite!(
    schema::axum::rpc_router,
    "procedure.echoOrder",
    "/rpc/procedure.echoOrder"
);
