//! FFI boundary helpers.
//!
//! The "Rust as real frontend, Flutter as UI only" architecture means the
//! Dart side sends serialized operations across the FFI boundary and reads
//! back serialized results. The wire format here is **JSON for now**
//! (CBOR can drop in later — it's the same serde-shaped types).
//!
//! What this module owns:
//!
//! - [`OperationRequest`] / [`OperationResponse`] / [`OperationError`] —
//!   the envelope every dispatched call goes through.
//! - [`json_request_into`] / [`json_response_from`] — narrow wrappers
//!   around `serde_json` so callers don't need to know the wire format.
//!
//! What this module **does not** own:
//!
//! - The dispatcher. A real FFI bridge needs a `match operation.model {
//!   "Account" => ..., "Tag" => ..., }` block, and that match knows the
//!   user's model types. It lives in the consumer's app crate, not here.
//!   The [`dispatch_example`] doctest shows the shape.

use serde::{Deserialize, Serialize};

/// The verb the FFI request is asking for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    FindMany,
    FindUnique,
    Create,
    Update,
    Delete,
}

/// A single operation crossing the FFI boundary.
///
/// `payload` is the per-operation argument — for `FindUnique` it's the PK,
/// for `Create` it's the create-input shape, etc. The consumer's dispatcher
/// destructures `payload` against the expected schema for `(model, kind)`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationRequest {
    pub model: String,
    pub kind: OperationKind,
    #[serde(default)]
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum OperationResponse {
    Ok { data: serde_json::Value },
    Err(OperationError),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationError {
    pub code: String,
    pub message: String,
}

impl OperationResponse {
    pub fn ok<T: Serialize>(value: &T) -> Result<Self, serde_json::Error> {
        Ok(Self::Ok {
            data: serde_json::to_value(value)?,
        })
    }

    pub fn err(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Err(OperationError {
            code: code.into(),
            message: message.into(),
        })
    }
}

impl From<crate::RusqliteError> for OperationResponse {
    fn from(error: crate::RusqliteError) -> Self {
        let code = match &error {
            crate::RusqliteError::NotFound => "not_found",
            crate::RusqliteError::Locked => "locked",
            crate::RusqliteError::Sqlite(_) => "sqlite",
            crate::RusqliteError::BatchTooLarge { .. } => "batch_too_large",
            crate::RusqliteError::DuplicateBatchKey { .. } => "duplicate_batch_key",
            crate::RusqliteError::Validation(_) => "validation",
        };
        Self::err(code, error.to_string())
    }
}

/// Decode a JSON FFI request from bytes. Use this from your FFI entry point
/// to parse the buffer Dart sends across.
pub fn json_request_from(bytes: &[u8]) -> Result<OperationRequest, serde_json::Error> {
    serde_json::from_slice(bytes)
}

/// Encode a response back to JSON bytes.
pub fn json_response_into(response: &OperationResponse) -> Vec<u8> {
    serde_json::to_vec(response).unwrap_or_else(|_| {
        // Serialization of OperationResponse itself can't realistically
        // fail (all variants are plain serde-derived types), but if it
        // ever does the FFI caller still needs a parseable error.
        br#"{"status":"err","code":"serialize","message":"response serialization failed"}"#.to_vec()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_round_trips_through_json() {
        let req = OperationRequest {
            model: "Account".into(),
            kind: OperationKind::FindUnique,
            payload: serde_json::json!({"id": 42}),
        };
        let bytes = serde_json::to_vec(&req).unwrap();
        let decoded = json_request_from(&bytes).unwrap();
        assert_eq!(decoded.model, "Account");
        assert_eq!(decoded.kind, OperationKind::FindUnique);
        assert_eq!(decoded.payload, req.payload);
    }

    #[test]
    fn response_ok_serializes_with_status_tag() {
        let resp = OperationResponse::ok(&serde_json::json!({"id": 1})).unwrap();
        let bytes = json_response_into(&resp);
        let text = std::str::from_utf8(&bytes).unwrap();
        assert!(text.contains("\"status\":\"ok\""));
        assert!(text.contains("\"id\":1"));
    }

    #[test]
    fn response_err_carries_code_and_message() {
        let resp = OperationResponse::err("not_found", "row missing");
        let bytes = json_response_into(&resp);
        let text = std::str::from_utf8(&bytes).unwrap();
        assert!(text.contains("\"status\":\"err\""));
        assert!(text.contains("\"code\":\"not_found\""));
    }

    #[test]
    fn rusqlite_error_maps_to_response_with_stable_code() {
        let resp: OperationResponse = crate::RusqliteError::NotFound.into();
        let bytes = json_response_into(&resp);
        assert!(std::str::from_utf8(&bytes).unwrap().contains("not_found"));
    }
}
