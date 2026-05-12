//! The FFI byte-boundary pattern, made concrete.
//!
//! Run with:
//!
//! ```sh
//! cargo run --example sqlite_ffi_dispatch -p cratestack
//! ```
//!
//! On a real mobile build, Dart calls into Rust through `flutter_rust_bridge`
//! (or a hand-rolled `cdylib`). What crosses the language boundary is just
//! bytes — there is no Rust type living on the Dart side. This example
//! shows the dispatcher you'd wire to that boundary:
//!
//!   `Dart writes JSON bytes  →  Rust decodes OperationRequest
//!     →  match on (model, kind)  →  delegate call
//!     →  encode OperationResponse  →  Dart reads JSON bytes`
//!
//! The dispatch table lives in the consumer's app crate because it needs
//! to know your specific model types. Treat this file as the template
//! to copy.

use cratestack::include_schema;
use cratestack::{RusqliteRuntime, rusqlite_backend::ddl::create_table_sql};
use cratestack_rusqlite::ffi::{
    OperationKind, OperationRequest, OperationResponse, json_request_from, json_response_into,
};
use cratestack_rusqlite::{ModelDelegate, RusqliteError};
use serde::Deserialize;

include_schema!("examples/sqlite_ffi_dispatch.cstack");

use cratestack_schema::models::Todo;
use cratestack_schema::{CreateTodoInput, TODO_MODEL, UpdateTodoInput};

/// The single FFI entry point. In a real app, the cdylib exports this
/// (or `flutter_rust_bridge` generates the wrapper around it). Bytes in,
/// bytes out — no Rust types cross the language boundary.
fn ffi_call(runtime: &RusqliteRuntime, bytes: &[u8]) -> Vec<u8> {
    let request = match json_request_from(bytes) {
        Ok(req) => req,
        Err(error) => {
            return json_response_into(&OperationResponse::err("bad_request", error.to_string()));
        }
    };
    json_response_into(&dispatch(runtime, request))
}

fn dispatch(runtime: &RusqliteRuntime, request: OperationRequest) -> OperationResponse {
    // The match below is the entire "router" for the on-device API.
    // Add a new model? Add a new arm. Add a new operation on an existing
    // model? Add a new branch inside that arm.
    if request.model != "Todo" {
        return OperationResponse::err(
            "unknown_model",
            format!("model `{}` is not exposed", request.model),
        );
    }

    let todos = ModelDelegate::<Todo, uuid::Uuid>::new(runtime, &TODO_MODEL);

    match request.kind {
        OperationKind::FindMany => match todos.find_many().run() {
            Ok(rows) => OperationResponse::ok(&rows).unwrap(),
            Err(error) => OperationResponse::from(error),
        },
        OperationKind::FindUnique => {
            let id: uuid::Uuid = match serde_json::from_value(request.payload) {
                Ok(id) => id,
                Err(error) => return OperationResponse::err("bad_input", error.to_string()),
            };
            match todos.find_unique(id).run() {
                Ok(Some(row)) => OperationResponse::ok(&row).unwrap(),
                Ok(None) => OperationResponse::err("not_found", "todo not found"),
                Err(error) => OperationResponse::from(error),
            }
        }
        OperationKind::Create => {
            let input: CreateTodoInput = match serde_json::from_value(request.payload) {
                Ok(input) => input,
                Err(error) => return OperationResponse::err("bad_input", error.to_string()),
            };
            match todos.create(input).run() {
                Ok(row) => OperationResponse::ok(&row).unwrap(),
                Err(error) => OperationResponse::from(error),
            }
        }
        OperationKind::Update => {
            #[derive(Deserialize)]
            #[allow(non_snake_case)]
            struct UpdatePayload {
                id: uuid::Uuid,
                input: UpdateTodoInput,
            }
            let payload: UpdatePayload = match serde_json::from_value(request.payload) {
                Ok(payload) => payload,
                Err(error) => return OperationResponse::err("bad_input", error.to_string()),
            };
            match todos.update(payload.id).set(payload.input).run() {
                Ok(row) => OperationResponse::ok(&row).unwrap(),
                Err(error) => OperationResponse::from(error),
            }
        }
        OperationKind::Delete => {
            let id: uuid::Uuid = match serde_json::from_value(request.payload) {
                Ok(id) => id,
                Err(error) => return OperationResponse::err("bad_input", error.to_string()),
            };
            match todos.delete(id).run() {
                Ok(row) => OperationResponse::ok(&row).unwrap(),
                Err(RusqliteError::NotFound) => {
                    OperationResponse::err("not_found", "todo not found")
                }
                Err(error) => OperationResponse::from(error),
            }
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = RusqliteRuntime::open_in_memory()?;
    runtime.with_connection(|conn| {
        conn.execute_batch(&create_table_sql(&TODO_MODEL))
            .expect("create todos");
        Ok(())
    })?;

    // ---- Simulate Dart sending a create request ----
    let id = uuid::Uuid::new_v4();
    let request_bytes = serde_json::to_vec(&serde_json::json!({
        "model": "Todo",
        "kind": "create",
        "payload": {
            "id": id,
            "title": "Buy milk",
            "completed": false,
            "createdAt": chrono::Utc::now(),
        },
    }))?;
    let response_bytes = ffi_call(&runtime, &request_bytes);
    println!(
        "create response: {}",
        std::str::from_utf8(&response_bytes)?
    );

    // ---- Simulate Dart fetching it back ----
    let request_bytes = serde_json::to_vec(&serde_json::json!({
        "model": "Todo",
        "kind": "find_unique",
        "payload": id,
    }))?;
    let response_bytes = ffi_call(&runtime, &request_bytes);
    println!(
        "\nfind_unique response: {}",
        std::str::from_utf8(&response_bytes)?
    );

    // ---- Simulate Dart toggling completed ----
    let request_bytes = serde_json::to_vec(&serde_json::json!({
        "model": "Todo",
        "kind": "update",
        "payload": {"id": id, "input": {"completed": true}},
    }))?;
    let response_bytes = ffi_call(&runtime, &request_bytes);
    println!(
        "\nupdate response: {}",
        std::str::from_utf8(&response_bytes)?
    );

    // ---- Simulate Dart sending malformed input (defence-in-depth) ----
    let response_bytes = ffi_call(&runtime, b"not valid json at all");
    println!(
        "\nbad-request response: {}",
        std::str::from_utf8(&response_bytes)?
    );

    Ok(())
}
