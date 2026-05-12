//! Demonstrates the FFI bridge pattern: a Dart side sends a JSON-encoded
//! `OperationRequest`, Rust dispatches it against a per-app model registry,
//! and returns a JSON-encoded `OperationResponse`. The actual FFI boundary
//! (cdylib + `flutter_rust_bridge` glue) is per-app and not in scope for
//! this crate; this test simulates the boundary using buffers.

use cratestack_rusqlite::{
    CreateModelInput, FromRusqliteRow, ModelDelegate, RusqliteRuntime, SqlColumnValue, SqlValue,
    UpdateModelInput,
    ddl::create_table_sql,
    ffi::{
        OperationKind, OperationRequest, OperationResponse, json_request_from, json_response_into,
    },
};
use cratestack_sql::{
    AuditConfig, AuthPolicies, LifecycleConfig, ModelColumn, ModelDescriptor, QueryCapabilities,
    TableMeta,
};
use rusqlite::Row;
use serde::{Deserialize, Serialize};

// --- model & schema --------------------------------------------------------
//
// In a real app these would be emitted by `include_schema!`. Here we declare
// them by hand so the test stays self-contained and exercises the FFI types
// independently of the macro.

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Tag {
    id: String,
    label: String,
}

impl FromRusqliteRow for Tag {
    fn from_rusqlite_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            label: row.get("label")?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CreateTagInput {
    id: String,
    label: String,
}

impl CreateModelInput<Tag> for CreateTagInput {
    fn sql_values(&self) -> Vec<SqlColumnValue> {
        vec![
            SqlColumnValue {
                column: "id",
                value: SqlValue::String(self.id.clone()),
            },
            SqlColumnValue {
                column: "label",
                value: SqlValue::String(self.label.clone()),
            },
        ]
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct UpdateTagInput {
    label: Option<String>,
}

impl UpdateModelInput<Tag> for UpdateTagInput {
    fn sql_values(&self) -> Vec<SqlColumnValue> {
        let mut values = Vec::new();
        if let Some(label) = &self.label {
            values.push(SqlColumnValue {
                column: "label",
                value: SqlValue::String(label.clone()),
            });
        }
        values
    }
}

const COLUMNS: &[ModelColumn] = &[
    ModelColumn {
        rust_name: "id",
        sql_name: "id",
    },
    ModelColumn {
        rust_name: "label",
        sql_name: "label",
    },
];

static TAG_DESCRIPTOR: ModelDescriptor<Tag, String> = ModelDescriptor::new(
    TableMeta {
        schema_name: "Tag",
        table_name: "tags",
        columns: COLUMNS,
        primary_key: "id",
    },
    QueryCapabilities {
        allowed_fields: &[],
        allowed_includes: &[],
        allowed_sorts: &[],
    },
    AuthPolicies {
        read_allow_policies: &[],
        read_deny_policies: &[],
        detail_allow_policies: &[],
        detail_deny_policies: &[],
        create_allow_policies: &[],
        create_deny_policies: &[],
        update_allow_policies: &[],
        update_deny_policies: &[],
        delete_allow_policies: &[],
        delete_deny_policies: &[],
    },
    AuditConfig {
        audit_enabled: false,
        pii_columns: &[],
        sensitive_columns: &[],
    },
    LifecycleConfig {
        create_defaults: &[],
        emitted_events: &[],
        version_column: None,
        soft_delete_column: None,
        retention_days: None,
    },
);

// --- the dispatcher --------------------------------------------------------
//
// This is the code that, in a real app, would live next to the FFI entry
// point. It matches on (model, kind) and routes to the right delegate call.

fn dispatch(runtime: &RusqliteRuntime, request: OperationRequest) -> OperationResponse {
    if request.model != "Tag" {
        return OperationResponse::err("unknown_model", format!("unknown model `{}`", request.model));
    }

    let delegate = ModelDelegate::<Tag, String>::new(runtime, &TAG_DESCRIPTOR);

    match request.kind {
        OperationKind::FindUnique => {
            let id: String = match serde_json::from_value(request.payload) {
                Ok(id) => id,
                Err(error) => return OperationResponse::err("bad_input", error.to_string()),
            };
            match delegate.find_unique(id).run() {
                Ok(Some(row)) => OperationResponse::ok(&row).unwrap(),
                Ok(None) => OperationResponse::err("not_found", "no such tag"),
                Err(error) => OperationResponse::from(error),
            }
        }
        OperationKind::FindMany => match delegate.find_many().run() {
            Ok(rows) => OperationResponse::ok(&rows).unwrap(),
            Err(error) => error.into(),
        },
        OperationKind::Create => {
            let input: CreateTagInput = match serde_json::from_value(request.payload) {
                Ok(input) => input,
                Err(error) => return OperationResponse::err("bad_input", error.to_string()),
            };
            match delegate.create(input).run() {
                Ok(row) => OperationResponse::ok(&row).unwrap(),
                Err(error) => OperationResponse::from(error),
            }
        }
        OperationKind::Update => {
            #[derive(Deserialize)]
            struct UpdatePayload {
                id: String,
                input: UpdateTagInput,
            }
            let payload: UpdatePayload = match serde_json::from_value(request.payload) {
                Ok(payload) => payload,
                Err(error) => return OperationResponse::err("bad_input", error.to_string()),
            };
            match delegate.update(payload.id).set(payload.input).run() {
                Ok(row) => OperationResponse::ok(&row).unwrap(),
                Err(error) => OperationResponse::from(error),
            }
        }
        OperationKind::Delete => {
            let id: String = match serde_json::from_value(request.payload) {
                Ok(id) => id,
                Err(error) => return OperationResponse::err("bad_input", error.to_string()),
            };
            match delegate.delete(id).run() {
                Ok(row) => OperationResponse::ok(&row).unwrap(),
                Err(error) => OperationResponse::from(error),
            }
        }
    }
}

/// The actual FFI entry point. In a real app this is what `flutter_rust_bridge`
/// calls. Bytes in, bytes out — no Rust types cross the language boundary.
fn ffi_call(runtime: &RusqliteRuntime, bytes: &[u8]) -> Vec<u8> {
    let request = match json_request_from(bytes) {
        Ok(req) => req,
        Err(error) => {
            return json_response_into(&OperationResponse::err("bad_request", error.to_string()));
        }
    };
    let response = dispatch(runtime, request);
    json_response_into(&response)
}

// --- tests -----------------------------------------------------------------

fn setup() -> RusqliteRuntime {
    let runtime = RusqliteRuntime::open_in_memory().expect("open in-memory sqlite");
    let ddl = create_table_sql(&TAG_DESCRIPTOR);
    runtime
        .with_connection(|conn| {
            conn.execute_batch(&ddl).expect("apply DDL");
            Ok(())
        })
        .unwrap();
    runtime
}

#[test]
fn create_then_find_unique_round_trips_through_ffi_bytes() {
    let runtime = setup();
    let create_bytes = serde_json::to_vec(&serde_json::json!({
        "model": "Tag",
        "kind": "create",
        "payload": {"id": "t-1", "label": "alpha"},
    }))
    .unwrap();
    let response = ffi_call(&runtime, &create_bytes);
    let parsed: serde_json::Value = serde_json::from_slice(&response).unwrap();
    assert_eq!(parsed["status"], "ok");
    assert_eq!(parsed["data"]["id"], "t-1");
    assert_eq!(parsed["data"]["label"], "alpha");

    let find_bytes = serde_json::to_vec(&serde_json::json!({
        "model": "Tag",
        "kind": "find_unique",
        "payload": "t-1",
    }))
    .unwrap();
    let response = ffi_call(&runtime, &find_bytes);
    let parsed: serde_json::Value = serde_json::from_slice(&response).unwrap();
    assert_eq!(parsed["status"], "ok");
    assert_eq!(parsed["data"]["label"], "alpha");
}

#[test]
fn find_unique_missing_returns_not_found_error() {
    let runtime = setup();
    let bytes = serde_json::to_vec(&serde_json::json!({
        "model": "Tag",
        "kind": "find_unique",
        "payload": "does-not-exist",
    }))
    .unwrap();
    let response = ffi_call(&runtime, &bytes);
    let parsed: serde_json::Value = serde_json::from_slice(&response).unwrap();
    assert_eq!(parsed["status"], "err");
    assert_eq!(parsed["code"], "not_found");
}

#[test]
fn malformed_request_returns_bad_request_error_without_panicking() {
    let runtime = setup();
    let response = ffi_call(&runtime, b"this is not json");
    let parsed: serde_json::Value = serde_json::from_slice(&response).unwrap();
    assert_eq!(parsed["status"], "err");
    assert_eq!(parsed["code"], "bad_request");
}

#[test]
fn unknown_model_returns_typed_error() {
    let runtime = setup();
    let bytes = serde_json::to_vec(&serde_json::json!({
        "model": "Account",
        "kind": "find_unique",
        "payload": "x",
    }))
    .unwrap();
    let response = ffi_call(&runtime, &bytes);
    let parsed: serde_json::Value = serde_json::from_slice(&response).unwrap();
    assert_eq!(parsed["code"], "unknown_model");
}

#[test]
fn update_via_ffi_round_trip() {
    let runtime = setup();
    let create_bytes = serde_json::to_vec(&serde_json::json!({
        "model": "Tag",
        "kind": "create",
        "payload": {"id": "u-1", "label": "old"},
    }))
    .unwrap();
    ffi_call(&runtime, &create_bytes);

    let update_bytes = serde_json::to_vec(&serde_json::json!({
        "model": "Tag",
        "kind": "update",
        "payload": {"id": "u-1", "input": {"label": "new"}},
    }))
    .unwrap();
    let response = ffi_call(&runtime, &update_bytes);
    let parsed: serde_json::Value = serde_json::from_slice(&response).unwrap();
    assert_eq!(parsed["status"], "ok");
    assert_eq!(parsed["data"]["label"], "new");
}

#[test]
fn find_many_returns_array_of_rows() {
    let runtime = setup();
    for (id, label) in [("a", "Alpha"), ("b", "Beta")] {
        let bytes = serde_json::to_vec(&serde_json::json!({
            "model": "Tag",
            "kind": "create",
            "payload": {"id": id, "label": label},
        }))
        .unwrap();
        ffi_call(&runtime, &bytes);
    }

    let list_bytes = serde_json::to_vec(&serde_json::json!({
        "model": "Tag",
        "kind": "find_many",
        "payload": null,
    }))
    .unwrap();
    let response = ffi_call(&runtime, &list_bytes);
    let parsed: serde_json::Value = serde_json::from_slice(&response).unwrap();
    assert_eq!(parsed["status"], "ok");
    let rows = parsed["data"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
}
