//! Native tests for the device app: the generated embedded model round-trips
//! a server-shaped row through `DescriptorModel`, and the unsynced marker
//! follows the ack. The wasm build runs this same code.

use chrono::Utc;
use cratestack_rusqlite::RusqliteRuntime;
use cratestack_sync::protocol::{
    ACK_MODEL, Change, ChangeOp, Cursor, PullResponse, RegisterResponse,
};
use serde_json::json;
use uuid::Uuid;

use super::NotesApp;
use crate::cratestack_schema::Note;

fn registered(user: &str) -> (NotesApp, Uuid) {
    let app = NotesApp::open(RusqliteRuntime::open_in_memory().unwrap(), user).unwrap();
    let client_id = Uuid::new_v4();
    app.with_engine(|engine, conn| {
        engine.apply_register(
            conn,
            RegisterResponse {
                client_id,
                mutation_ttl_secs: 60,
            },
        )
    })
    .unwrap();
    (app, client_id)
}

#[test]
fn offline_writes_are_visible_and_marked_unsynced() {
    let (app, _) = registered("alice");
    let id = app.create("Buy milk").unwrap();
    app.update(id, None, Some(true)).unwrap();
    let notes = app.notes().unwrap();
    assert_eq!(notes.len(), 1);
    assert!(notes[0].done && notes[0].unsynced);
    let status = app.status().unwrap();
    assert_eq!((status.pending, status.unpushed), (2, 2));
}

#[test]
fn a_server_row_lands_through_the_generated_model_and_the_ack_clears_the_marker() {
    let (app, client_id) = registered("alice");
    let id = app.create("Buy milk").unwrap();
    let request = app
        .with_engine(|engine, conn| engine.push_request(conn, 10))
        .unwrap()
        .unwrap();
    assert_eq!(request.mutations[0].name, "note.create");

    // What the server's fetcher sends: its generated `Note`, serialized.
    let server_row = serde_json::to_value(Note {
        id,
        ownerId: "alice".into(),
        title: "Buy milk".into(),
        done: false,
        updatedAt: Utc::now(),
    })
    .unwrap();
    let response = PullResponse {
        changes: vec![
            Change {
                model: "Note".into(),
                pk: id.to_string(),
                op: ChangeOp::Upsert { row: server_row },
            },
            Change {
                model: ACK_MODEL.into(),
                pk: client_id.to_string(),
                op: ChangeOp::Upsert {
                    row: json!({ "seq": 1 }),
                },
            },
        ],
        cursor: Cursor::new(1, 1),
        has_more: false,
    };
    app.with_engine(|engine, conn| engine.apply_pull(conn, response))
        .unwrap();
    let notes = app.notes().unwrap();
    assert_eq!(notes.len(), 1);
    assert!(!notes[0].unsynced);
    assert_eq!(app.status().unwrap().pending, 0);

    // And a server delete removes it.
    let response = PullResponse {
        changes: vec![Change {
            model: "Note".into(),
            pk: id.to_string(),
            op: ChangeOp::Delete,
        }],
        cursor: Cursor::new(2, 1),
        has_more: false,
    };
    app.with_engine(|engine, conn| engine.apply_pull(conn, response))
        .unwrap();
    assert!(app.notes().unwrap().is_empty());
}
