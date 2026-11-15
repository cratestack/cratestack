//! End-to-end proof of the per-event SSE cap (cratestack#1379): a real axum
//! server that says `ready` and then streams one event that never ends.
//! The client's `subscribe` must end with `EventTooLarge` and drop the
//! connection, which the server sees as its response body being dropped.

use std::convert::Infallible;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{Response, header};
use axum::routing::get;
use bytes::Bytes;
use cratestack_client_rust::{
    ClientConfig, ClientError, CratestackClient, RpcClientError, SubscriptionTarget,
    SubscriptionTransport,
};
use cratestack_codec_cbor::CborCodec;
use url::Url;

struct DropFlag(Arc<AtomicBool>);

impl Drop for DropFlag {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

async fn endless(State(dropped): State<Arc<AtomicBool>>) -> Response<Body> {
    let guard = DropFlag(dropped);
    let stream = async_stream::stream! {
        let _guard = guard;
        yield Ok::<_, Infallible>(Bytes::from_static(b"event: ready\ndata: {\"id\":1,\"ready\":{}}\n\n"));
        loop {
            yield Ok(Bytes::from(vec![b'x'; 1024]));
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    };
    Response::builder()
        .header(header::CONTENT_TYPE, "text/event-stream")
        .body(Body::from_stream(stream))
        .expect("response should build")
}

#[tokio::test]
async fn an_endless_event_ends_the_subscription_and_drops_the_connection() {
    let dropped = Arc::new(AtomicBool::new(false));
    let app = Router::new()
        .route("/rpc/subscribe/procedure.endless", get(endless))
        .with_state(dropped.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await });

    let config = ClientConfig::new(Url::parse(&format!("http://{addr}")).unwrap())
        .with_subscription_transport(SubscriptionTransport::Sse)
        .with_max_sse_event_bytes(16 * 1024);
    let client = CratestackClient::new(config, CborCodec);
    let target = SubscriptionTarget {
        op_id: "procedure.endless".to_owned(),
        input: serde_json::json!({}),
        sse_path: "/rpc/subscribe/procedure.endless".to_owned(),
        sse_query: None,
        socket_path: "/rpc/ws",
    };

    let mut subscription = client.subscribe::<serde_json::Value>(target).await.unwrap();
    let refusal = tokio::time::timeout(Duration::from_secs(10), subscription.recv())
        .await
        .expect("the client must stop reading, not buffer forever")
        .expect("an error, not a clean end");
    assert!(
        matches!(refusal, Err(ClientError::EventTooLarge { limit: 16384 })),
        "{refusal:?}"
    );
    assert!(
        subscription.recv().await.is_none(),
        "nothing after the refusal"
    );
    for _ in 0..100 {
        if dropped.load(Ordering::SeqCst) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the server's response body was never dropped: the connection stayed open");
}

async fn oversize_ready() -> Response<Body> {
    let body = format!("event: ready\ndata: {}\n\n", "x".repeat(4096));
    Response::builder()
        .header(header::CONTENT_TYPE, "text/event-stream")
        .body(Body::from(body))
        .expect("response should build")
}

/// The `ready` event itself over the cap: the open call fails with the typed
/// error, and the RPC client surfaces it as its own typed variant rather than
/// an `InvalidResponse` string.
#[tokio::test]
async fn an_oversize_ready_is_a_typed_error_on_the_rest_and_the_rpc_surface() {
    let app = Router::new().route("/rpc/subscribe/procedure.big", get(oversize_ready));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await });
    let config = ClientConfig::new(Url::parse(&format!("http://{addr}")).unwrap())
        .with_subscription_transport(SubscriptionTransport::Sse)
        .with_max_sse_event_bytes(1024);
    let client = CratestackClient::new(config, CborCodec);
    let target = SubscriptionTarget {
        op_id: "procedure.big".to_owned(),
        input: serde_json::json!({}),
        sse_path: "/rpc/subscribe/procedure.big".to_owned(),
        sse_query: None,
        socket_path: "/rpc/ws",
    };
    let Err(error) = client.subscribe::<serde_json::Value>(target).await else {
        panic!("an oversize ready must refuse the open");
    };
    assert!(
        matches!(error, ClientError::EventTooLarge { limit: 1024 }),
        "{error:?}"
    );
    assert!(matches!(
        RpcClientError::from(error),
        RpcClientError::EventTooLarge { limit: 1024 }
    ));
}
