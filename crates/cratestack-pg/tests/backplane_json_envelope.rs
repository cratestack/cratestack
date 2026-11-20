//! The rolling-deploy option `PgNotifyBackplane::with_json_envelope`: a
//! 0.20.6 replica writing what a 0.20.5 replica reads. Its own test binary
//! because the option is process-wide (the writers that hold no backplane
//! share it), so it must not leak into the CBOR tests.
//!
//! Postgres-backed: run it with `CRATESTACK_REQUIRE_DB=1` (a skip reports
//! `0.00s`).

use std::sync::Arc;
use std::time::Duration;

use cratestack::sqlx::postgres::{PgListener, PgPoolOptions};
use cratestack::{BackplaneMessage, PgNotifyBackplane, SubscriptionBackplane};
use futures_util::StreamExt;
use serde::Deserialize;

mod support;

use support::pg;

/// 0.20.5's notification type, as it parsed one.
#[derive(Deserialize)]
struct Legacy {
    c: String,
    p: Option<String>,
    r: Option<uuid::Uuid>,
}

#[tokio::test]
async fn the_json_envelope_is_what_an_older_reader_parses_and_a_newer_one_reads() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let _guard = pg::serial_guard().await;
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&test_pg.url)
        .await
        .unwrap();
    let writer = PgNotifyBackplane::new(pool.clone()).with_json_envelope();
    // A default replica (CBOR writer) reads the JSON one's notifications.
    let reader = Arc::new(PgNotifyBackplane::new(pool.clone()));
    let mut on_reader = reader.subscribe("rollout").await.unwrap();
    let mut raw = PgListener::connect(&test_pg.url).await.unwrap();
    raw.listen("cratestack_events").await.unwrap();

    writer.publish("rollout", "{\"n\":1}").await.unwrap();
    writer
        .publish_bytes("rollout", b"\x00\xffbytes")
        .await
        .unwrap();

    // What the 0.20.5 parser sees.
    let text = raw.recv().await.unwrap();
    let old: Legacy = serde_json::from_str(text.payload()).expect("legacy-parseable JSON");
    assert_eq!(
        (old.c.as_str(), old.p.as_deref()),
        ("rollout", Some("{\"n\":1}"))
    );
    let bytes = raw.recv().await.unwrap();
    let old: Legacy = serde_json::from_str(bytes.payload()).expect("legacy-parseable JSON");
    assert!(
        old.p.is_none() && old.r.is_none(),
        "nothing an old reader acts on"
    );

    // What the new reader gets.
    let next = async |receiver: &mut cratestack::BackplaneReceiver| {
        tokio::time::timeout(Duration::from_secs(5), receiver.next())
            .await
            .expect("a message within 5s")
            .expect("open")
    };
    assert_eq!(
        next(&mut on_reader).await,
        BackplaneMessage::Payload("{\"n\":1}".into())
    );
    assert_eq!(
        next(&mut on_reader).await,
        BackplaneMessage::Bytes(Arc::from(&b"\x00\xffbytes"[..]))
    );
}
