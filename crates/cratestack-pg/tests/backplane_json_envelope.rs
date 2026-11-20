//! The rolling-deploy switch `PgNotifyBackplane::use_json_envelope()`: a
//! 0.20.6 process writing what a 0.20.5 replica reads. Its own test binary
//! because the switch is process-wide and cannot be undone in-process, so it
//! must not leak into the CBOR tests; and one test, because what it checks
//! is the order: what was written before the switch is CBOR, and everything
//! after it is JSON, whichever backplane or writer made it.
//!
//! Postgres-backed: run it with `CRATESTACK_REQUIRE_DB=1` (a skip reports
//! `0.00s`).

use std::sync::Arc;
use std::time::Duration;

use cratestack::sqlx::PgPool;
use cratestack::sqlx::postgres::{PgListener, PgPoolOptions};
use cratestack::{BackplaneMessage, PgNotifyBackplane, SubscriptionBackplane, Topic};
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

async fn raw(listener: &mut PgListener) -> String {
    tokio::time::timeout(Duration::from_secs(5), listener.recv())
        .await
        .expect("a notification within 5s")
        .expect("recv")
        .payload()
        .to_owned()
}

/// What a 0.20.5 replica makes of `text`: the notification it would act on.
fn as_0_20_5_reads(text: &str) -> Legacy {
    serde_json::from_str(text).unwrap_or_else(|error| panic!("not legacy JSON ({error}): {text}"))
}

async fn write_everything(
    backplane: &PgNotifyBackplane,
    pool: &PgPool,
    listener: &mut PgListener,
    channel: &str,
) -> Vec<String> {
    backplane.publish(channel, "{\"n\":1}").await.unwrap();
    backplane.publish_bytes(channel, b"\x00\xff").await.unwrap();
    let mut texts = vec![raw(listener).await, raw(listener).await];
    // A topic's `publish_in_tx`, which holds no backplane writer of its own.
    let topic =
        Topic::<serde_json::Value>::new(Arc::new(PgNotifyBackplane::new(pool.clone())), "T", "k");
    let mut tx = pool.begin().await.unwrap();
    topic
        .publish_in_tx(&mut tx, &serde_json::json!({"in": "tx"}))
        .await
        .unwrap();
    tx.commit().await.unwrap();
    texts.push(raw(listener).await);
    // A model event's outbox write.
    cratestack_sqlx::enqueue_event_outbox(
        pool,
        "Widget",
        cratestack::ModelEventKind::Created,
        &serde_json::json!({"id": 1}),
    )
    .await
    .unwrap();
    texts.push(raw(listener).await);
    texts
}

#[tokio::test]
async fn once_switched_every_writer_in_the_process_writes_what_an_older_reader_parses() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let _guard = pg::serial_guard().await;
    let pool = PgPoolOptions::new()
        .max_connections(3)
        .connect(&test_pg.url)
        .await
        .unwrap();
    cratestack_sqlx::ensure_event_outbox_table(&pool)
        .await
        .unwrap();
    let mut listener = PgListener::connect(&test_pg.url).await.unwrap();
    listener.listen("cratestack_events").await.unwrap();
    let before = PgNotifyBackplane::new(pool.clone());

    // Before the switch: CBOR, and the switch is not retroactive.
    before.publish("early", "{}").await.unwrap();
    assert!(raw(&mut listener).await.starts_with("cbor:"));

    PgNotifyBackplane::use_json_envelope();
    let after = PgNotifyBackplane::new(pool.clone());

    // A backplane built before the call, one built after it, and the writers
    // that hold no backplane: all JSON, all parseable by a 0.20.5 reader.
    for (who, backplane) in [("before", &before), ("after", &after)] {
        let texts = write_everything(backplane, &pool, &mut listener, "rollout").await;
        for text in &texts {
            assert!(text.starts_with('{'), "{who}: {text}");
            as_0_20_5_reads(text);
        }
        let (text, bytes, topic, model) = (&texts[0], &texts[1], &texts[2], &texts[3]);
        let old = as_0_20_5_reads(text);
        assert_eq!(
            (old.c.as_str(), old.p.as_deref()),
            ("rollout", Some("{\"n\":1}"))
        );
        let old = as_0_20_5_reads(bytes);
        assert!(
            old.p.is_none() && old.r.is_none(),
            "nothing an old reader acts on"
        );
        assert_eq!(as_0_20_5_reads(topic).c, "cratestack.topic.T.k");
        let old = as_0_20_5_reads(model);
        assert_eq!(old.c, "cratestack.model.Widget");
        assert!(old.p.is_some(), "the model event travels inline");
    }

    // And a default replica still reads the JSON ones, bytes included.
    let reader = Arc::new(PgNotifyBackplane::new(pool.clone()));
    let mut on_reader = reader.subscribe("rollout.read").await.unwrap();
    after.publish("rollout.read", "{\"n\":2}").await.unwrap();
    after
        .publish_bytes("rollout.read", b"\x00\xffbytes")
        .await
        .unwrap();
    for expected in [
        BackplaneMessage::Payload("{\"n\":2}".into()),
        BackplaneMessage::Bytes(Arc::from(&b"\x00\xffbytes"[..])),
    ] {
        let got = tokio::time::timeout(Duration::from_secs(5), on_reader.next())
            .await
            .expect("a message within 5s")
            .expect("open");
        assert_eq!(got, expected);
    }
    // The wrapper is the same switch.
    let _ = PgNotifyBackplane::new(pool.clone()).with_json_envelope();
}
