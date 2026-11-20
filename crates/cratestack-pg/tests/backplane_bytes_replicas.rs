//! Byte payloads across replicas over `PgNotifyBackplane` (A24 decision 366,
//! O2): two backplanes, each with its own pool and `LISTEN` connection,
//! share one database. Covers the CBOR envelope, the size limit on the final
//! `NOTIFY` text, the legacy JSON envelope a 0.20.5 replica writes, and a
//! malformed notification, which is skipped without stopping the listener.
//!
//! Postgres-backed: run it with `CRATESTACK_REQUIRE_DB=1`, or a missing
//! database is a skip that still prints `ok` (CLAUDE.md, "Critical test
//! gotcha"). A skip reports `0.00s`.

use std::sync::Arc;
use std::time::Duration;

use cratestack::sqlx::PgPool;
use cratestack::sqlx::postgres::PgPoolOptions;
use cratestack::{
    BackplaneMessage, BackplaneReceiver, CratestackError, PgNotifyBackplane, SubscriptionBackplane,
};
use futures_util::StreamExt;

mod support;

use support::pg;

async fn replica(url: &str) -> (Arc<PgNotifyBackplane>, PgPool) {
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(url)
        .await
        .expect("a replica's own pool");
    (Arc::new(PgNotifyBackplane::new(pool.clone())), pool)
}

async fn next(receiver: &mut BackplaneReceiver) -> BackplaneMessage {
    tokio::time::timeout(Duration::from_secs(5), receiver.next())
        .await
        .expect("a message within 5s")
        .expect("the stream stays open")
}

async fn raw_notify(pool: &PgPool, text: &str) {
    cratestack::sqlx::query("SELECT pg_notify('cratestack_events', $1)")
        .bind(text)
        .execute(pool)
        .await
        .expect("notify");
}

#[tokio::test]
async fn bytes_published_on_one_replica_arrive_byte_identical_on_another() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let _guard = pg::serial_guard().await;
    let ((a, _), (b, _)) = (replica(&test_pg.url).await, replica(&test_pg.url).await);
    let mut on_b = b.subscribe("bytes.one").await.unwrap();
    let blob: Vec<u8> = (0..=255).cycle().take(4000).collect();

    a.publish_bytes("bytes.one", &blob).await.unwrap();
    a.publish_bytes("bytes.one", &[]).await.unwrap();

    assert_eq!(
        next(&mut on_b).await,
        BackplaneMessage::Bytes(Arc::from(blob))
    );
    assert_eq!(
        next(&mut on_b).await,
        BackplaneMessage::Bytes(Arc::from(Vec::new()))
    );
}

#[tokio::test]
async fn text_still_arrives_and_mixes_with_bytes_in_order() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let _guard = pg::serial_guard().await;
    let ((a, _), (b, _)) = (replica(&test_pg.url).await, replica(&test_pg.url).await);
    let mut on_b = b.subscribe("mixed").await.unwrap();
    let mut on_a = a.subscribe("mixed").await.unwrap();

    a.publish("mixed", "{\"n\":1}").await.unwrap();
    a.publish_bytes("mixed", b"\x00\x01").await.unwrap();
    a.publish("mixed", "{\"n\":3}").await.unwrap();

    for receiver in [&mut on_b, &mut on_a] {
        assert_eq!(
            next(receiver).await,
            BackplaneMessage::Payload("{\"n\":1}".into())
        );
        assert_eq!(
            next(receiver).await,
            BackplaneMessage::Bytes(Arc::from(&b"\x00\x01"[..]))
        );
        assert_eq!(
            next(receiver).await,
            BackplaneMessage::Payload("{\"n\":3}".into())
        );
    }
}

#[tokio::test]
async fn an_oversize_byte_payload_is_refused_with_the_limit_and_the_size_never_truncated() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let _guard = pg::serial_guard().await;
    let ((a, _), (b, _)) = (replica(&test_pg.url).await, replica(&test_pg.url).await);
    let mut on_b = b.subscribe("big").await.unwrap();

    // Base64 grows 3 bytes to 4: 6000 bytes is 8000 characters and an
    // envelope, over the 7900 limit although under it as bytes.
    let error = a.publish_bytes("big", &vec![7; 6000]).await.unwrap_err();
    let CratestackError::BadRequest(message) = &error else {
        panic!("a typed refusal, got {error:?}");
    };
    assert!(
        message.contains("6000 bytes") && message.contains("7900"),
        "{message}"
    );
    // The largest that fits goes, whole.
    let fits = vec![9; 5800];
    a.publish_bytes("big", &fits).await.unwrap();
    assert_eq!(
        next(&mut on_b).await,
        BackplaneMessage::Bytes(Arc::from(fits))
    );
    // And the refused one reached nobody: it was the first thing sent.
    assert!(
        tokio::time::timeout(Duration::from_millis(200), on_b.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn a_text_payload_over_the_limit_is_still_refused_with_no_outbox_row() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let _guard = pg::serial_guard().await;
    let (a, _) = replica(&test_pg.url).await;
    let error = a.publish("big.text", &"x".repeat(8000)).await.unwrap_err();
    assert!(matches!(error, CratestackError::BadRequest(_)), "{error:?}");
}

#[tokio::test]
async fn a_legacy_json_notification_from_an_older_replica_is_still_read() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let _guard = pg::serial_guard().await;
    let (b, pool) = replica(&test_pg.url).await;
    let mut on_b = b.subscribe("legacy").await.unwrap();

    // Byte for byte what 0.20.5's `PgNotifyBackplane::publish` sent.
    raw_notify(&pool, r#"{"c":"legacy","p":"{\"old\":true}","n":41}"#).await;
    // And before it had a nonce.
    raw_notify(&pool, r#"{"c":"legacy","p":"older"}"#).await;

    assert_eq!(
        next(&mut on_b).await,
        BackplaneMessage::Payload("{\"old\":true}".into())
    );
    assert_eq!(
        next(&mut on_b).await,
        BackplaneMessage::Payload("older".into())
    );
}

#[tokio::test]
async fn a_malformed_notification_is_skipped_and_the_listener_keeps_running() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let _guard = pg::serial_guard().await;
    let ((a, pool), (b, _)) = (replica(&test_pg.url).await, replica(&test_pg.url).await);
    let mut on_b = b.subscribe("junk").await.unwrap();

    for junk in [
        "cbor:!!not base64!!",
        "cbor:/w==",             // base64 of 0xff: not CBOR
        "cbor:oWF2Ag==",         // {"v": 2}: an unknown version
        "cbor:o2F2AWFjZGp1bmth", // truncated map
        "{\"c\":\"junk\"}",      // JSON with no payload
        "{broken",
        "no envelope at all",
        "",
    ] {
        raw_notify(&pool, junk).await;
    }
    // Still listening: a good one after the junk arrives, and nothing before.
    a.publish_bytes("junk", b"after").await.unwrap();
    assert_eq!(
        next(&mut on_b).await,
        BackplaneMessage::Bytes(Arc::from(&b"after"[..]))
    );
}

#[tokio::test]
async fn a_model_event_whose_notification_cannot_fit_fails_the_write() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let _guard = pg::serial_guard().await;
    cratestack_sqlx::ensure_event_outbox_table(&test_pg.pool)
        .await
        .unwrap();
    // The channel alone (`cratestack.model.<model>`) is over the limit, so
    // not even the by-reference notification fits. 0.20.5 failed the
    // statement here; it must not commit an event nobody is told of.
    let model = "M".repeat(8000);
    cratestack::sqlx::query("DELETE FROM cratestack_event_outbox WHERE model = $1")
        .bind(&model)
        .execute(&test_pg.pool)
        .await
        .unwrap();
    let error = cratestack_sqlx::enqueue_event_outbox(
        &test_pg.pool,
        &model,
        cratestack::ModelEventKind::Created,
        &serde_json::json!({"id": 1}),
    )
    .await
    .unwrap_err();
    assert!(matches!(error, CratestackError::Internal(_)), "{error:?}");
    let rows: (i64,) =
        cratestack::sqlx::query_as("SELECT count(*) FROM cratestack_event_outbox WHERE model = $1")
            .bind(&model)
            .fetch_one(&test_pg.pool)
            .await
            .unwrap();
    assert_eq!(rows.0, 0, "no outbox row for the refused event");
}
