//! Two replicas sharing one database (`docs/design/subscriptions-websocket.md`
//! §4, slice S2): a write committed through replica A reaches an SSE
//! subscriber connected to replica B. Each replica has its own pool and so its
//! own `PgNotifyBackplane` listener; B never drains A's outbox, so the event
//! can only have crossed through Postgres `NOTIFY`. The read policy still
//! applies on B: owner 2 receives nothing of owner 1's.

use std::time::Duration;

use cratestack::axum::http::StatusCode;
use cratestack::sqlx::postgres::PgPoolOptions;
use cratestack::{CratestackContext, Value};

use super::support::pg;
use super::{HeaderCaller, NoProcedures, cratestack_schema, delivered_ids, received, subscribe};

#[tokio::test]
async fn a_write_on_one_replica_reaches_a_subscriber_on_another() {
    let Some(test_pg) = pg::connect_or_skip().await else {
        return;
    };
    let _guard = pg::serial_guard().await;
    for statement in [
        "DROP TABLE IF EXISTS sub_notes",
        "CREATE TABLE sub_notes (id BIGINT PRIMARY KEY, owner_id BIGINT NOT NULL, body TEXT NOT NULL, archived BOOLEAN NOT NULL)",
        "DROP TABLE IF EXISTS cratestack_event_outbox",
    ] {
        cratestack::sqlx::query(statement)
            .execute(&test_pg.pool)
            .await
            .expect(statement);
    }
    let replica_a = cratestack_schema::Cratestack::builder(test_pg.pool.clone()).build();
    let pool_b = PgPoolOptions::new()
        .max_connections(2)
        .connect(&test_pg.url)
        .await
        .expect("replica B's own pool");
    let replica_b = cratestack_schema::Cratestack::builder(pool_b).build();
    let router_b = cratestack_schema::axum::rpc_router(
        replica_b,
        NoProcedures,
        (),
        cratestack_codec_cbor::CborCodec,
        HeaderCaller,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    );

    let op = "model.SubNote.subscribe";
    let owner_one = subscribe(&router_b, op, Some(1)).await;
    let owner_two = subscribe(&router_b, op, Some(2)).await;
    assert_eq!(owner_one.status(), StatusCode::OK);
    assert_eq!(owner_two.status(), StatusCode::OK);

    replica_a
        .sub_note()
        .bind(CratestackContext::authenticated([(
            "id".to_owned(),
            Value::Int(1),
        )]))
        .create(cratestack_schema::CreateSubNoteInput {
            id: 1,
            ownerId: 1,
            body: "written on A".to_owned(),
            archived: false,
        })
        .run()
        .await
        .expect("note created on replica A");

    let window = Duration::from_millis(1500);
    let (owner_one, owner_two) =
        tokio::join!(received(owner_one, window), received(owner_two, window));
    assert!(
        owner_one.starts_with("event: ready"),
        "B's stream opens with ready: {owner_one}"
    );
    assert_eq!(delivered_ids(&owner_one), [1], "owner 1 on B: {owner_one}");
    assert_eq!(
        delivered_ids(&owner_two),
        Vec::<i64>::new(),
        "owner 2 on B: {owner_two}"
    );
}
