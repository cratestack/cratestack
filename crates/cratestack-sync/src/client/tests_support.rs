//! Fixtures for the client tests: an in-memory database and a deliberately
//! non-idempotent mutator (`inc`). Replaying `inc` twice double-counts, so
//! any test built on it fails loudly if pending work is re-applied on top of
//! state that already includes it.

use cratestack_rusqlite::rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::client::{ClientError, SyncEngine, SyncedModel};
use crate::protocol::{ACK_MODEL, Change, ChangeOp, Cursor, PullResponse, RegisterResponse};

pub(super) struct Counters;

impl SyncedModel for Counters {
    fn model(&self) -> &str {
        "Counter"
    }
    fn table(&self) -> &str {
        "counters"
    }
    fn pk_column(&self) -> &str {
        "id"
    }
    fn upsert(&self, conn: &Connection, row: &Value) -> Result<(), ClientError> {
        conn.execute(
            "INSERT INTO counters (id, value) VALUES (?1, ?2) \
             ON CONFLICT (id) DO UPDATE SET value = excluded.value",
            params![row["id"].as_str(), row["value"].as_i64()],
        )?;
        Ok(())
    }
    fn delete(&self, conn: &Connection, pk: &str) -> Result<(), ClientError> {
        conn.execute("DELETE FROM counters WHERE id = ?1", [pk])?;
        Ok(())
    }
}

pub(super) fn inc(conn: &Connection, args: &Value) -> Result<(), ClientError> {
    conn.execute(
        "INSERT INTO counters (id, value) VALUES (?1, 1) \
         ON CONFLICT (id) DO UPDATE SET value = value + 1",
        [args["id"].as_str()],
    )?;
    Ok(())
}

pub(super) fn setup() -> (Connection, SyncEngine, Uuid) {
    let mut conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("CREATE TABLE counters (id TEXT PRIMARY KEY, value INTEGER NOT NULL)")
        .unwrap();
    let engine = SyncEngine::new().model(Counters).mutation("inc", inc);
    engine.install(&mut conn).unwrap();
    let client_id = Uuid::new_v4();
    engine
        .apply_register(
            &mut conn,
            RegisterResponse {
                client_id,
                mutation_ttl_secs: 60,
            },
        )
        .unwrap();
    (conn, engine, client_id)
}

pub(super) fn value(conn: &Connection, id: &str) -> Option<i64> {
    conn.query_row("SELECT value FROM counters WHERE id = ?1", [id], |r| {
        r.get(0)
    })
    .optional()
    .unwrap()
}

pub(super) fn pull(changes: Vec<Change>) -> PullResponse {
    PullResponse {
        changes,
        cursor: Cursor::new(10, 1),
        has_more: false,
    }
}

pub(super) fn upsert(id: &str, value: i64) -> Change {
    Change {
        model: "Counter".into(),
        pk: id.into(),
        op: ChangeOp::Upsert {
            row: json!({ "id": id, "value": value }),
        },
    }
}

pub(super) fn ack(client_id: Uuid, seq: u64) -> Change {
    Change {
        model: ACK_MODEL.into(),
        pk: client_id.to_string(),
        op: ChangeOp::Upsert {
            row: json!({ "seq": seq }),
        },
    }
}
