//! Wire types that mirror the Studio API's JSON contract. Kept thin
//! on purpose: the UI uses serde_json::Value for record payloads so
//! we don't have to grow a parallel schema-Aware type set just to
//! display rows in a table.
//!
//! Some fields (e.g. `is_id`, `primary_key`, `has_api`) are
//! deserialized but unused in Phase 1b — they're shipped now so the
//! upcoming write-path UI in Phase 3 doesn't have to revisit
//! deserialization. `#[allow(dead_code)]` covers the gap.

#![allow(dead_code)]

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct TargetSummary {
    pub key: String,
    pub display_name: String,
    pub mode: String,
    pub has_db: bool,
    pub has_api: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TargetList {
    pub workspace: String,
    pub targets: Vec<TargetSummary>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FieldSummary {
    pub name: String,
    pub type_name: String,
    pub arity: String,
    pub is_id: bool,
    pub is_relation: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelSummary {
    pub name: String,
    pub primary_key: Option<String>,
    pub fields: Vec<FieldSummary>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelList {
    pub models: Vec<ModelSummary>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Page {
    pub rows: Vec<serde_json::Map<String, serde_json::Value>>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RecordResponse {
    pub row: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SnippetResponse {
    pub rust: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApiError {
    pub error: ApiErrorBody,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApiErrorBody {
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub fields: Vec<FieldError>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FieldError {
    pub field: String,
    pub code: String,
    pub message: String,
}

/// Untagged follow-response: either a single row (Required-arity
/// relation) or a paginated page (List-arity relation).
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum FollowResponse {
    Single {
        row: Option<serde_json::Map<String, serde_json::Value>>,
    },
    Page(Page),
}
