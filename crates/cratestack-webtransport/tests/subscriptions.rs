//! One subscription per stream: Ready, items, a terminal frame, FIN — and a
//! client cancel that reaches the dispatcher.

mod support;

use std::sync::atomic::Ordering;
use std::time::Duration;

use cratestack_webtransport::{JSON_PROTOCOL, WebTransportLimits, WireCodec};
use support::{Client, start};

#[tokio::test]
async fn ready_then_items_then_end_then_fin() {
    let server = start(WebTransportLimits::default());
    let client = server.session(Some("7")).await;
    let mut stream = client
        .subscribe(1, "test.items", serde_json::json!({ "n": 3 }))
        .await;
    assert!(stream.next().await.unwrap().ready.is_some());
    for i in 0..3 {
        let frame = stream.next().await.unwrap();
        assert_eq!(
            (frame.id, frame.next),
            (1, Some(serde_json::json!({ "i": i })))
        );
    }
    assert!(stream.next().await.unwrap().end.is_some());
    assert!(
        stream.next().await.is_none(),
        "FIN after the terminal frame"
    );
}

#[tokio::test]
async fn a_refused_open_is_an_error_before_ready() {
    let server = start(WebTransportLimits::default());
    let client = server.session(Some("7")).await;
    let mut stream = client
        .subscribe(2, "test.refuse", serde_json::Value::Null)
        .await;
    let frame = stream.next().await.unwrap();
    assert_eq!(
        frame.err.as_ref().map(|e| e.code.as_str()),
        Some("permission_denied")
    );
    assert!(stream.next().await.is_none());
}

#[tokio::test]
async fn a_mid_stream_error_is_terminal() {
    let server = start(WebTransportLimits::default());
    let client = server.session(Some("7")).await;
    let mut stream = client
        .subscribe(3, "test.fail_mid", serde_json::Value::Null)
        .await;
    assert!(stream.next().await.unwrap().ready.is_some());
    assert_eq!(
        stream.next().await.unwrap().next,
        Some(serde_json::json!("first"))
    );
    let frame = stream.next().await.unwrap();
    assert_eq!(
        frame.err.as_ref().map(|e| e.code.as_str()),
        Some("unavailable")
    );
    assert!(stream.next().await.is_none());
}

#[tokio::test]
async fn a_client_cancel_drops_the_dispatcher_stream() {
    let server = start(WebTransportLimits::default());
    let client = server.session(Some("7")).await;
    let mut stream = client
        .subscribe(4, "test.forever", serde_json::Value::Null)
        .await;
    assert!(stream.next().await.unwrap().ready.is_some());
    assert!(stream.next().await.unwrap().next.is_some());
    assert!(!server.dispatcher.dropped.load(Ordering::SeqCst));
    stream.recv.stop(0).unwrap();
    for _ in 0..100 {
        if server.dispatcher.dropped.load(Ordering::SeqCst) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the dispatcher stream outlived the client's cancel by 1s");
}

#[tokio::test]
async fn many_subscriptions_share_one_session() {
    let server = start(WebTransportLimits::default());
    let client = server.session(Some("7")).await;
    let mut streams = Vec::new();
    for id in 0..32 {
        streams.push(
            client
                .subscribe(id, "test.items", serde_json::json!({ "n": 5 }))
                .await,
        );
    }
    for mut stream in streams {
        let mut items = 0;
        while let Some(frame) = stream.next().await {
            items += usize::from(frame.next.is_some());
        }
        assert_eq!(items, 5);
    }
}

#[tokio::test]
async fn the_json_codec_is_negotiated_by_subprotocol() {
    let server = start(WebTransportLimits::default());
    let request = wtq_request(&server.url).with_protocol(JSON_PROTOCOL);
    let session = server.connect_with(request).await.unwrap();
    assert_eq!(session.protocol(), Some(JSON_PROTOCOL));
    let mut client = Client::new(session, WireCodec::Json).await;
    assert!(client.init(Default::default()).await.ready.is_some());
    let mut stream = client
        .subscribe(1, "test.items", serde_json::json!({ "n": 1 }))
        .await;
    assert!(stream.next().await.unwrap().ready.is_some());
    assert!(stream.next().await.unwrap().next.is_some());
}

#[tokio::test]
async fn the_codec_query_parameter_works_without_subprotocols() {
    let server = start(WebTransportLimits::default());
    let session = server
        .connect_with(wtq_request(&format!("{}?codec=json", server.url)))
        .await
        .unwrap();
    assert_eq!(
        session.protocol(),
        None,
        "nothing negotiated, nothing echoed"
    );
    let mut client = Client::new(session, WireCodec::Json).await;
    assert!(client.init(Default::default()).await.ready.is_some());
}

fn wtq_request(url: &str) -> web_transport_quinn::proto::ConnectRequest {
    web_transport_quinn::proto::ConnectRequest::new(url.parse::<url::Url>().unwrap())
}
