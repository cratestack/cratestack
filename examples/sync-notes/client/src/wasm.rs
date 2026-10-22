//! `wasm-bindgen` surface for the demo page's workers. JSON strings in and
//! out: the protocol types are already serde, so the worker can hand the
//! server's response body straight through without a second schema.

use std::cell::RefCell;

use cratestack_rusqlite::RusqliteRuntime;
use cratestack_rusqlite::opfs::{OpfsOptions, install_opfs_vfs};
use cratestack_sync::client::ClientError;
use serde::Serialize;
use uuid::Uuid;
use wasm_bindgen::prelude::*;

use crate::app::NotesApp;

thread_local! {
    static APP: RefCell<Option<NotesApp>> = const { RefCell::new(None) };
}

fn js(error: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&error.to_string())
}

fn json<T: Serialize>(value: &T) -> Result<String, JsValue> {
    serde_json::to_string(value).map_err(js)
}

fn parse<T: serde::de::DeserializeOwned>(body: &str) -> Result<T, JsValue> {
    serde_json::from_str(body).map_err(js)
}

fn with_app<T>(f: impl FnOnce(&NotesApp) -> Result<T, ClientError>) -> Result<T, JsValue> {
    APP.with(|cell| match cell.borrow().as_ref() {
        Some(app) => f(app).map_err(js),
        None => Err(js("call open() first")),
    })
}

/// Open the device database. Each device on the page passes its own OPFS
/// directory: a SyncAccessHandle pool is exclusive to one worker. Returns
/// `"opfs"`, or `"memory"` when OPFS is unavailable (data then lasts only
/// as long as the worker).
#[wasm_bindgen]
pub async fn open(user: String, opfs_directory: String) -> Result<String, JsValue> {
    console_error_panic_hook::set_once();
    let options = OpfsOptions {
        directory: opfs_directory,
        ..OpfsOptions::default()
    };
    let (runtime, storage) = match install_opfs_vfs(&options).await {
        Ok(()) => (RusqliteRuntime::open("notes.db").map_err(js)?, "opfs"),
        Err(_) => (RusqliteRuntime::open_in_memory().map_err(js)?, "memory"),
    };
    let app = NotesApp::open(runtime, &user).map_err(js)?;
    APP.with(|cell| *cell.borrow_mut() = Some(app));
    Ok(storage.into())
}

#[wasm_bindgen]
pub fn notes() -> Result<String, JsValue> {
    json(&with_app(|app| app.notes())?)
}

#[wasm_bindgen]
pub fn create(title: String) -> Result<String, JsValue> {
    Ok(with_app(|app| app.create(&title))?.to_string())
}

#[wasm_bindgen]
pub fn update(id: String, title: Option<String>, done: Option<bool>) -> Result<(), JsValue> {
    let id: Uuid = id.parse().map_err(js)?;
    with_app(|app| app.update(id, title.as_deref(), done))
}

#[wasm_bindgen]
pub fn remove(id: String) -> Result<(), JsValue> {
    let id: Uuid = id.parse().map_err(js)?;
    with_app(|app| app.delete(id))
}

#[wasm_bindgen]
pub fn status() -> Result<String, JsValue> {
    json(&with_app(|app| app.status())?)
}

#[wasm_bindgen(js_name = clearRejected)]
pub fn clear_rejected() -> Result<(), JsValue> {
    with_app(|app| app.with_engine(|engine, conn| engine.clear_rejected(conn)))
}

#[wasm_bindgen(js_name = registerRequest)]
pub fn register_request(label: String) -> Result<String, JsValue> {
    json(&with_app(|app| {
        app.with_engine(|engine, _| Ok(engine.register_request(Some(label))))
    })?)
}

#[wasm_bindgen(js_name = applyRegister)]
pub fn apply_register(body: String) -> Result<(), JsValue> {
    let response = parse(&body)?;
    with_app(|app| app.with_engine(|engine, conn| engine.apply_register(conn, response)))
}

/// `null` when there is nothing to send (or not registered yet).
#[wasm_bindgen(js_name = pushRequest)]
pub fn push_request(max: usize) -> Result<Option<String>, JsValue> {
    let request = with_app(|app| app.with_engine(|engine, conn| engine.push_request(conn, max)))?;
    request.as_ref().map(json).transpose()
}

#[wasm_bindgen(js_name = applyPush)]
pub fn apply_push(body: String) -> Result<String, JsValue> {
    let response = parse(&body)?;
    json(&with_app(|app| {
        app.with_engine(|engine, conn| engine.apply_push(conn, response))
    })?)
}

#[wasm_bindgen(js_name = pullRequest)]
pub fn pull_request() -> Result<Option<String>, JsValue> {
    let request = with_app(|app| app.with_engine(|engine, conn| engine.pull_request(conn)))?;
    request.as_ref().map(json).transpose()
}

#[wasm_bindgen(js_name = applyPull)]
pub fn apply_pull(body: String) -> Result<String, JsValue> {
    let response = parse(&body)?;
    json(&with_app(|app| {
        app.with_engine(|engine, conn| engine.apply_pull(conn, response))
    })?)
}

/// Feed a `SyncErrorBody` from any endpoint; returns the `Recovery` action.
#[wasm_bindgen]
pub fn recover(body: String) -> Result<String, JsValue> {
    let error = parse(&body)?;
    json(&with_app(|app| {
        app.with_engine(|engine, conn| engine.recover(conn, &error))
    })?)
}
