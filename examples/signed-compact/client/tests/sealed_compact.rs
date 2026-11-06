//! End to end: the real server router, in-process on an ephemeral port,
//! driven by the real generated client. What crossed the wire comes from the
//! server's observer, which sits outside the envelope layer.

use std::sync::{Arc, Mutex};

use signed_compact_client::{DEMO_SECRET, client, schema, transfer};
use signed_compact_server::{Options, WireCall, app};
use url::Url;

const COMPACT: &str = "application/vnd.cratestack.compact+cbor";

type Calls = Arc<Mutex<Vec<WireCall>>>;

async fn serve(compact: bool) -> (Url, Calls) {
    let calls: Calls = Arc::default();
    let seen = calls.clone();
    let router = app(
        DEMO_SECRET,
        Options {
            compact,
            observer: Some(Arc::new(move |call| seen.lock().unwrap().push(call))),
        },
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        signed_compact_server::axum::serve(listener, router)
            .await
            .unwrap();
    });
    (Url::parse(&format!("http://{addr}")).unwrap(), calls)
}

fn assert_round_trip(back: &schema::Transaction) {
    let sent = transfer().input;
    assert_eq!(back.fromAccount, sent.fromAccount);
    assert_eq!(back.toAccount, sent.toAccount);
    assert_eq!(back.amountMinor, 12_500);
    assert_eq!(back.currency, "EUR");
    assert_eq!(back.reference, sent.reference);
    assert_eq!(back.status, "settled");
}

#[tokio::test]
async fn a_sealed_client_is_answered_compact_then_seals_compact_requests() {
    let (base, calls) = serve(true).await;
    let client = client(base, DEMO_SECRET, true);

    let first = client
        .procedures()
        .transfer_funds(&transfer())
        .await
        .unwrap();
    let second = client
        .procedures()
        .transfer_funds(&transfer())
        .await
        .unwrap();

    assert_round_trip(&first);
    assert_round_trip(&second);
    assert_ne!(first.id, second.id);

    let calls = calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    // Call 1: named request (no payload type = plain CBOR), compact answer.
    assert_eq!(calls[0].request, "");
    assert_eq!(calls[0].response, COMPACT);
    // Call 2: the request is sealed under the compact type too.
    assert_eq!(calls[1].request, COMPACT);
    assert_eq!(calls[1].response, COMPACT);
    // The outer envelope is COSE both ways, every time.
    for call in calls.iter() {
        assert!(call.request_content_type.starts_with("application/cose"));
        assert!(call.response_content_type.starts_with("application/cose"));
    }
}

#[tokio::test]
async fn a_sealed_client_stays_named_against_a_server_that_did_not_opt_in() {
    let (base, calls) = serve(false).await;
    let client = client(base, DEMO_SECRET, true);

    for _ in 0..2 {
        let back = client
            .procedures()
            .transfer_funds(&transfer())
            .await
            .unwrap();
        assert_round_trip(&back);
    }

    let calls = calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    // Never a compact request, so never an unsigned 415 to recover from.
    for call in calls.iter() {
        assert_eq!(call.request, "");
        assert_ne!(call.response, COMPACT);
        assert!(call.response_content_type.starts_with("application/cose"));
    }
}
