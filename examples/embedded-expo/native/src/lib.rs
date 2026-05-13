#![allow(non_snake_case)]

//! Rust side of the `embedded-expo` example.
//!
//! Exposes a single C-ABI entry point — `cratestack_dispatch` — that the
//! iOS / Android Expo native module calls. The wire format is the same
//! JSON envelope used by `cratestack_rusqlite::ffi`: bytes in, bytes
//! out, no Rust types cross the language boundary. The JS side
//! constructs an `OperationRequest`, ferries it to native, and decodes
//! the `OperationResponse` that comes back.
//!
//! The shape of the dispatcher is intentionally tiny — it's just a
//! `match (model, kind)` against the cratestack-generated
//! `ModelDelegate`. New models are new match arms, new operations are
//! new branches inside an existing arm.

mod schema {
    use cratestack_macros::include_embedded_schema;
    include_embedded_schema!("notes.cstack");
}

use std::ffi::{c_char, CStr};
use std::path::PathBuf;
use std::ptr;
use std::sync::OnceLock;

use chrono::Utc;
use cratestack_rusqlite::ddl::create_table_sql;
use cratestack_rusqlite::ffi::{
    OperationKind, OperationRequest, OperationResponse, json_request_from, json_response_into,
};
use cratestack_rusqlite::{ModelDelegate, RusqliteRuntime};
use serde::Deserialize;
use uuid::Uuid;

static RUNTIME: OnceLock<RusqliteRuntime> = OnceLock::new();

#[derive(Deserialize)]
struct CreateInput {
    title: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    pinned: bool,
}

#[derive(Deserialize)]
struct UpdateInput {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    pinned: Option<bool>,
    #[serde(default)]
    completed: Option<bool>,
}

fn note_delegate(
    runtime: &RusqliteRuntime,
) -> ModelDelegate<'_, schema::cratestack_schema::Note, Uuid> {
    ModelDelegate::new(runtime, &schema::cratestack_schema::NOTE_MODEL)
}

/// Open the SQLite file (idempotent) and bootstrap the Note table.
///
/// This is the only call that needs to happen out-of-band before the
/// dispatcher accepts requests. The native module calls it once at
/// app startup with the platform's per-app data directory path.
pub fn init_database(db_path: &str) -> Result<(), String> {
    if RUNTIME.get().is_some() {
        return Ok(());
    }
    let path = PathBuf::from(db_path);
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
    }
    let opened = RusqliteRuntime::open(&path).map_err(|error| error.to_string())?;
    opened
        .with_connection(|conn| {
            conn.execute_batch(&create_table_sql(&schema::cratestack_schema::NOTE_MODEL))?;
            Ok(())
        })
        .map_err(|error| error.to_string())?;
    let _ = RUNTIME.set(opened);
    Ok(())
}

/// Dispatch against the global `OnceLock`-managed runtime. The C ABI
/// wrapper below threads its bytes through here.
pub fn dispatch(bytes: &[u8]) -> Vec<u8> {
    let runtime = match RUNTIME.get() {
        Some(runtime) => runtime,
        None => {
            return json_response_into(&OperationResponse::err(
                "not_initialized",
                "init_database must be called before dispatch",
            ));
        }
    };
    dispatch_against(runtime, bytes)
}

/// Dispatch against an explicit runtime. Useful for tests that want a
/// fresh in-memory DB per test (the `OnceLock`-backed global path
/// otherwise pins the first DB across the whole test binary).
pub fn dispatch_against(runtime: &RusqliteRuntime, bytes: &[u8]) -> Vec<u8> {
    let request = match json_request_from(bytes) {
        Ok(req) => req,
        Err(error) => {
            return json_response_into(&OperationResponse::err("bad_request", error.to_string()));
        }
    };
    json_response_into(&route(runtime, request))
}

fn route(runtime: &RusqliteRuntime, request: OperationRequest) -> OperationResponse {
    if request.model != "Note" {
        return OperationResponse::err(
            "unknown_model",
            format!("model `{}` is not exposed", request.model),
        );
    }
    let notes = note_delegate(runtime);
    match request.kind {
        OperationKind::FindMany => match notes
            .find_many()
            .order_by(schema::cratestack_schema::note::updatedAt().desc())
            .limit(500)
            .run()
        {
            Ok(rows) => OperationResponse::ok(&rows).unwrap_or_else(|error| {
                OperationResponse::err("serialize_failure", error.to_string())
            }),
            Err(error) => OperationResponse::from(error),
        },
        OperationKind::FindUnique => {
            let id: Uuid = match serde_json::from_value(request.payload) {
                Ok(id) => id,
                Err(error) => return OperationResponse::err("bad_input", error.to_string()),
            };
            match notes.find_unique(id).run() {
                Ok(Some(row)) => OperationResponse::ok(&row).unwrap_or_else(|error| {
                    OperationResponse::err("serialize_failure", error.to_string())
                }),
                Ok(None) => OperationResponse::err("not_found", "note not found"),
                Err(error) => OperationResponse::from(error),
            }
        }
        OperationKind::Create => {
            let input: CreateInput = match serde_json::from_value(request.payload) {
                Ok(input) => input,
                Err(error) => return OperationResponse::err("bad_input", error.to_string()),
            };
            let now = Utc::now();
            match notes
                .create(schema::cratestack_schema::CreateNoteInput {
                    id: Uuid::new_v4(),
                    title: input.title,
                    body: input.body,
                    pinned: input.pinned,
                    completed: false,
                    createdAt: now,
                    updatedAt: now,
                })
                .run()
            {
                Ok(row) => OperationResponse::ok(&row).unwrap_or_else(|error| {
                    OperationResponse::err("serialize_failure", error.to_string())
                }),
                Err(error) => OperationResponse::from(error),
            }
        }
        OperationKind::Update => {
            #[derive(Deserialize)]
            struct UpdateEnvelope {
                id: Uuid,
                #[serde(flatten)]
                patch: UpdateInput,
            }
            let envelope: UpdateEnvelope = match serde_json::from_value(request.payload) {
                Ok(envelope) => envelope,
                Err(error) => return OperationResponse::err("bad_input", error.to_string()),
            };
            let now = Utc::now();
            match notes
                .update(envelope.id)
                .set(schema::cratestack_schema::UpdateNoteInput {
                    title: envelope.patch.title,
                    body: envelope.patch.body,
                    pinned: envelope.patch.pinned,
                    completed: envelope.patch.completed,
                    updatedAt: Some(now),
                    ..Default::default()
                })
                .run()
            {
                Ok(row) => OperationResponse::ok(&row).unwrap_or_else(|error| {
                    OperationResponse::err("serialize_failure", error.to_string())
                }),
                Err(error) => OperationResponse::from(error),
            }
        }
        OperationKind::Delete => {
            let id: Uuid = match serde_json::from_value(request.payload) {
                Ok(id) => id,
                Err(error) => return OperationResponse::err("bad_input", error.to_string()),
            };
            match notes.delete(id).run() {
                Ok(row) => OperationResponse::ok(&row).unwrap_or_else(|error| {
                    OperationResponse::err("serialize_failure", error.to_string())
                }),
                Err(error) => OperationResponse::from(error),
            }
        }
    }
}

// -----------------------------------------------------------------------------
// C ABI exports — what iOS (Swift) and Android (Kotlin via JNI) actually call.
// -----------------------------------------------------------------------------

/// `extern "C"` initializer. `db_path` is a NUL-terminated UTF-8 string
/// in the caller's address space. Returns `0` on success, `-1` on
/// failure. Failure reason is currently lost on this boundary; for a
/// production build, route errors through the same envelope as
/// `cratestack_dispatch`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cratestack_init(db_path: *const c_char) -> i32 {
    if db_path.is_null() {
        return -1;
    }
    let cstr = unsafe { CStr::from_ptr(db_path) };
    let path = match cstr.to_str() {
        Ok(s) => s,
        Err(_) => return -1,
    };
    match init_database(path) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// Invoke the JSON dispatcher.
///
/// - Inputs: a length-prefixed byte buffer (`in_ptr`/`in_len`).
/// - Output: the response is heap-allocated; `*out_ptr` receives the
///   pointer, `*out_len` receives the length. The caller must hand
///   the pointer back to `cratestack_free` to release it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cratestack_dispatch(
    in_ptr: *const u8,
    in_len: usize,
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
) {
    if in_ptr.is_null() || out_ptr.is_null() || out_len.is_null() {
        if !out_ptr.is_null() {
            unsafe {
                *out_ptr = ptr::null_mut();
                if !out_len.is_null() {
                    *out_len = 0;
                }
            }
        }
        return;
    }
    let input = unsafe { std::slice::from_raw_parts(in_ptr, in_len) };
    let response = dispatch(input);
    let mut boxed = response.into_boxed_slice();
    unsafe {
        *out_ptr = boxed.as_mut_ptr();
        *out_len = boxed.len();
    }
    std::mem::forget(boxed);
}

/// Release a buffer previously handed back by `cratestack_dispatch`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cratestack_free(ptr: *mut u8, len: usize) {
    if ptr.is_null() || len == 0 {
        return;
    }
    unsafe {
        let _ = Vec::from_raw_parts(ptr, len, len);
    }
}

// -----------------------------------------------------------------------------
// JNI bindings for the Android path. iOS uses the C ABI above directly via
// Swift `@_silgen_name`. Android's `System.loadLibrary` resolves Java methods
// against symbols of the form `Java_<dotted_class>_<methodName>`, so we
// expose two such symbols that thin-wrap `init_database` + `dispatch`.
// -----------------------------------------------------------------------------

#[cfg(target_os = "android")]
mod android_jni {
    use super::{dispatch, init_database};
    use jni::objects::{JByteArray, JClass, JString};
    use jni::sys::{jbyteArray, jint};
    use jni::JNIEnv;

    /// Matches Kotlin's
    /// `private external fun nativeInit(dbPath: String): Int`
    /// on class `dev.cratestack.examples.cratestacknotes.CratestackNotesModule`.
    #[unsafe(no_mangle)]
    pub extern "system" fn Java_dev_cratestack_examples_cratestacknotes_CratestackNotesModule_nativeInit<'local>(
        mut env: JNIEnv<'local>,
        _class: JClass<'local>,
        db_path: JString<'local>,
    ) -> jint {
        let path: String = match env.get_string(&db_path) {
            Ok(s) => s.into(),
            Err(_) => return -1,
        };
        match init_database(&path) {
            Ok(()) => 0,
            Err(_) => -1,
        }
    }

    /// Matches Kotlin's
    /// `private external fun nativeDispatch(request: ByteArray): ByteArray`.
    #[unsafe(no_mangle)]
    pub extern "system" fn Java_dev_cratestack_examples_cratestacknotes_CratestackNotesModule_nativeDispatch<'local>(
        mut env: JNIEnv<'local>,
        _class: JClass<'local>,
        request: JByteArray<'local>,
    ) -> jbyteArray {
        let bytes = match env.convert_byte_array(&request) {
            Ok(b) => b,
            Err(_) => Vec::new(),
        };
        let response = dispatch(&bytes);
        match env.byte_array_from_slice(&response) {
            Ok(arr) => arr.into_raw(),
            Err(_) => std::ptr::null_mut(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cratestack_rusqlite::ffi::{OperationKind, OperationRequest};
    use serde_json::json;

    /// Spin up a fresh in-memory SQLite + bootstrap the Note table for
    /// each test. We bypass the global `OnceLock`-backed runtime so
    /// tests don't share state.
    fn fresh_runtime() -> RusqliteRuntime {
        let runtime = RusqliteRuntime::open_in_memory().expect("in-memory db opens");
        runtime
            .with_connection(|conn| {
                conn.execute_batch(&create_table_sql(&schema::cratestack_schema::NOTE_MODEL))?;
                Ok(())
            })
            .expect("Note table bootstraps");
        runtime
    }

    fn dispatch_json(runtime: &RusqliteRuntime, request: &OperationRequest) -> serde_json::Value {
        let bytes = serde_json::to_vec(request).unwrap();
        let response_bytes = dispatch_against(runtime, &bytes);
        serde_json::from_slice(&response_bytes).unwrap()
    }

    #[test]
    fn create_then_find_unique_through_dispatcher() {
        let runtime = fresh_runtime();
        let create_response = dispatch_json(
            &runtime,
            &OperationRequest {
                model: "Note".into(),
                kind: OperationKind::Create,
                payload: json!({
                    "title": "Hello from JNI",
                    "body": "",
                    "pinned": false
                }),
            },
        );
        assert_eq!(create_response["status"], "ok", "response: {create_response}");
        let created_id = create_response["data"]["id"]
            .as_str()
            .expect("created note must have a string id")
            .to_owned();

        let find_response = dispatch_json(
            &runtime,
            &OperationRequest {
                model: "Note".into(),
                kind: OperationKind::FindUnique,
                payload: json!(created_id),
            },
        );
        assert_eq!(find_response["status"], "ok");
        assert_eq!(find_response["data"]["title"], "Hello from JNI");
    }

    #[test]
    fn unknown_model_returns_error_envelope() {
        let runtime = fresh_runtime();
        let response = dispatch_json(
            &runtime,
            &OperationRequest {
                model: "Account".into(),
                kind: OperationKind::FindMany,
                payload: serde_json::Value::Null,
            },
        );
        assert_eq!(response["status"], "err");
        assert_eq!(response["code"], "unknown_model");
    }

    #[test]
    fn dispatch_without_init_yields_not_initialized() {
        // The OnceLock-backed global RUNTIME stays empty across the
        // other tests in this binary (they use `dispatch_against` with
        // an explicit runtime). So a `dispatch()` call here lands on
        // the "not initialized" branch deterministically.
        let response_bytes = dispatch(
            &serde_json::to_vec(&OperationRequest {
                model: "Note".into(),
                kind: OperationKind::FindMany,
                payload: serde_json::Value::Null,
            })
            .unwrap(),
        );
        let response: serde_json::Value = serde_json::from_slice(&response_bytes).unwrap();
        assert_eq!(response["status"], "err");
        assert_eq!(response["code"], "not_initialized");
    }
}
