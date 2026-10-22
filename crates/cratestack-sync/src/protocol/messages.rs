//! Request and response bodies for `register`, `push` and `pull`.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::Cursor;

/// The model name the server uses for a client's own acknowledgement
/// change. Never a real model: `.cstack` model names cannot start with `$`.
pub const ACK_MODEL: &str = "$ack";

/// The private scope carrying one client's acknowledgements.
pub fn client_scope(client_id: Uuid) -> String {
    format!("client:{client_id}")
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterRequest {
    /// Free-form label for operators ("alice's laptop"); never trusted.
    #[serde(default)]
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterResponse {
    /// Server-issued, bound to the registering principal for its lifetime.
    pub client_id: Uuid,
    /// Echoed so the client can mirror the server's TTL locally.
    pub mutation_ttl_secs: i64,
}

/// One named, client-originated write. The name selects a server handler;
/// `args` are untrusted input to it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mutation {
    /// Per-client, contiguous from 1.
    pub seq: u64,
    pub name: String,
    pub args: Value,
    /// Client clock. Only used for the mutation TTL, which protects the
    /// client's own stale intent — never for ordering or authorization.
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushRequest {
    pub client_id: Uuid,
    pub mutations: Vec<Mutation>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushResponse {
    /// Highest seq the server has consumed (applied *or* rejected).
    pub last_mutation_seq: u64,
    pub results: Vec<MutationResult>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MutationResult {
    pub seq: u64,
    #[serde(flatten)]
    pub outcome: MutationOutcome,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "camelCase")]
pub enum MutationOutcome {
    Applied,
    /// A retry of a seq this client already pushed. Not re-executed.
    AlreadyApplied,
    /// Consumed but not applied. The client must drop the mutation's local
    /// effect; it keeps the mutation for the user to review.
    Rejected {
        code: RejectCode,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectCode {
    /// `@version` mismatch or another optimistic-concurrency failure.
    Conflict,
    /// The principal's policy does not allow this write.
    Forbidden,
    NotFound,
    /// Args failed validation.
    Invalid,
    /// Older than the mutation TTL.
    Expired,
    /// No server handler is registered under this name.
    UnknownMutation,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequest {
    pub client_id: Uuid,
    #[serde(default)]
    pub cursor: Cursor,
    /// Capped by the server's `max_pull_changes`.
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullResponse {
    pub changes: Vec<Change>,
    pub cursor: Cursor,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    pub model: String,
    /// Primary key, as text.
    pub pk: String,
    #[serde(flatten)]
    pub op: ChangeOp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum ChangeOp {
    /// The row as the puller may read it *now* — fetched at pull time
    /// through the model's read policy, not snapshotted at write time.
    Upsert { row: Value },
    /// Deleted, soft-deleted, moved out of the puller's scope, or no longer
    /// readable by the puller. The client cannot and need not tell which.
    Delete,
}
