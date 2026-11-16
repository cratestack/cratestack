//! The `application/cbor-seq` subscription reader end to end (cratestack#1001)
//! against a real axum server: the request asks for the media type, items
//! keep their CBOR shapes, `end` closes cleanly, and a body that just stops is
//! an error, not a quiet end.

mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use axum::Router;
use axum::routing::get;
use cratestack_client_rust::ClientError;

use support::{ID, Reading, app, client, endless, target};

#[tokio::test]
async fn items_keep_their_cbor_shapes_and_end_closes_the_stream() {
    let client = client(app(), 1 << 20).await;
    let mut subscription = client.subscribe::<Reading>(target("served")).await.unwrap();
    let want = Reading {
        id: uuid::Uuid::from_u128(ID),
        n: 7,
    };
    assert_eq!(subscription.recv().await.unwrap().unwrap(), want);
    assert_eq!(subscription.recv().await.unwrap().unwrap(), want);
    assert!(subscription.recv().await.is_none(), "`end` is a clean end");
}

#[tokio::test]
async fn an_err_frame_is_the_last_item() {
    let client = client(app(), 1 << 20).await;
    let mut subscription = client
        .subscribe::<Reading>(target("errored"))
        .await
        .unwrap();
    let error = subscription.recv().await.unwrap().unwrap_err();
    assert!(
        matches!(error, ClientError::Remote { status, .. } if status == 503),
        "{error:?}"
    );
    assert!(subscription.recv().await.is_none());
}

#[tokio::test]
async fn a_body_that_stops_without_a_terminal_frame_is_an_error() {
    let client = client(app(), 1 << 20).await;
    let mut subscription = client.subscribe::<Reading>(target("stops")).await.unwrap();
    assert!(subscription.recv().await.unwrap().is_ok());
    let error = subscription.recv().await.unwrap().unwrap_err();
    assert!(
        matches!(error, ClientError::InvalidResponse(_)),
        "{error:?}"
    );
    assert!(subscription.recv().await.is_none());
}

#[tokio::test]
async fn a_truncated_frame_at_the_end_is_an_error() {
    let client = client(app(), 1 << 20).await;
    let mut subscription = client
        .subscribe::<Reading>(target("truncated"))
        .await
        .unwrap();
    let error = subscription.recv().await.unwrap().unwrap_err();
    assert!(
        matches!(&error, ClientError::InvalidResponse(m) if m.contains("middle of a frame")),
        "{error:?}"
    );
}

#[tokio::test]
async fn an_endless_frame_ends_with_event_too_large_and_drops_the_connection() {
    let dropped = Arc::new(AtomicBool::new(false));
    let app = Router::new()
        .route("/rpc/subscribe/procedure.endless", get(endless))
        .with_state(dropped.clone());
    let client = client(app, 16 * 1024).await;
    let mut subscription = client
        .subscribe::<Reading>(target("endless"))
        .await
        .unwrap();
    let refusal = tokio::time::timeout(Duration::from_secs(10), subscription.recv())
        .await
        .expect("the client must stop reading, not buffer forever")
        .expect("an error, not a clean end");
    assert!(
        matches!(refusal, Err(ClientError::EventTooLarge { limit: 16384 })),
        "{refusal:?}"
    );
    assert!(subscription.recv().await.is_none());
    for _ in 0..100 {
        if dropped.load(Ordering::SeqCst) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the server's response body was never dropped: the connection stayed open");
}

#[tokio::test]
async fn an_sse_body_answered_to_a_cbor_seq_request_is_refused_by_its_media_type() {
    let client = client(app(), 1 << 20).await;
    let error = client
        .subscribe::<Reading>(target("wrong_media_type"))
        .await
        .map(|_| ())
        .unwrap_err();
    assert!(
        matches!(&error, ClientError::InvalidResponse(m) if m.contains("text/event-stream")),
        "{error:?}"
    );
}

#[tokio::test]
async fn the_media_type_is_matched_without_case_and_ignoring_parameters() {
    let client = client(app(), 1 << 20).await;
    let mut subscription = client
        .subscribe::<Reading>(target("media_type_params"))
        .await
        .unwrap();
    assert!(subscription.recv().await.is_none());
}

#[tokio::test]
async fn a_stream_whose_first_frame_is_not_ready_is_refused() {
    let client = client(app(), 1 << 20).await;
    let error = client
        .subscribe::<Reading>(target("next_first"))
        .await
        .map(|_| ())
        .unwrap_err();
    assert!(
        matches!(&error, ClientError::InvalidResponse(m) if m.contains("`ready`")),
        "{error:?}"
    );
}

#[tokio::test]
async fn an_err_before_ready_is_the_open_refusal() {
    let client = client(app(), 1 << 20).await;
    let error = client
        .subscribe::<Reading>(target("err_first"))
        .await
        .map(|_| ())
        .unwrap_err();
    assert!(
        matches!(error, ClientError::Remote { status, .. } if status == 403),
        "{error:?}"
    );
}
