//! Batch envelope.
//!
//! tRPC-style per-item envelope for batch operations. The HTTP
//! response is always `200 OK` carrying a [`BatchResponse<T>`];
//! whole-batch infrastructure failures (bad request shape, size cap
//! exceeded, DB connection lost) still flow through the outer
//! `Result<_, CoolError>` and map to their usual status codes via
//! the standard handler.
//!
//! Per-item failures (validation, policy denial, not-found, stale
//! `if_match`, PK conflict) ride inside [`BatchItemStatus::Error`]
//! and DO NOT abort the batch — successful items in the same request
//! still commit. The transactional model used by the server backends
//! is one outer transaction with a per-item SAVEPOINT, so failed
//! items leave no audit row, no event outbox entry, and no row
//! mutation.

use serde::{Deserialize, Serialize};

use crate::error::CoolError;

#[cfg(test)]
mod tests;

/// Per-item result inside a [`BatchResponse`]. The `index` is the
/// item's position in the original request, so clients can pair
/// results with inputs even after server-side reordering (e.g.
/// parallel `batch_get` fetches in the future).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BatchItemResult<T> {
    pub index: usize,
    #[serde(flatten)]
    pub status: BatchItemStatus<T>,
}

/// Either a successful per-item outcome (`Ok`) or a per-item failure
/// (`Error`). Serializes as a tagged enum with the discriminant in
/// `status`:
///
/// ```json
/// { "status": "ok",    "value": { ... } }
/// { "status": "error", "error": { "code": "POLICY_DENIED", "message": "…" } }
/// ```
///
/// The `code` field maps 1:1 to [`CoolError::code`], so consumers
/// can share error-code constants across single and batch routes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum BatchItemStatus<T> {
    Ok {
        value: T,
    },
    Error {
        error: BatchItemError,
    },
}

/// Public, safe-to-expose shape of a per-item failure. Mirrors
/// [`crate::CoolErrorResponse`] without the optional `details` field
/// — batch callers asking for per-item detail can repeat the
/// operation singly against the failed item to get the full error
/// envelope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BatchItemError {
    pub code: String,
    pub message: String,
}

impl BatchItemError {
    /// Project a [`CoolError`] into the public per-item shape, using
    /// the same `code()` / `public_message()` mapping the standard
    /// HTTP error handler uses for single-route responses.
    pub fn from_cool(error: &CoolError) -> Self {
        Self {
            code: error.code().to_owned(),
            message: error.public_message().into_owned(),
        }
    }
}

/// Summary counts attached to every [`BatchResponse`] so callers can
/// branch on aggregate status without scanning the result list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchSummary {
    pub total: usize,
    pub ok: usize,
    pub err: usize,
}

/// Wire envelope returned by every batch route. Always `200 OK` at
/// the HTTP layer; inspect `summary.err` (or scan `results`) to
/// surface per-item failures to the user.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BatchResponse<T> {
    pub results: Vec<BatchItemResult<T>>,
    pub summary: BatchSummary,
}

impl<T> BatchResponse<T> {
    /// Build a [`BatchResponse`] from an in-order
    /// `Vec<Result<T, CoolError>>`. The `index` of each result
    /// matches its position in the input.
    pub fn from_results(per_item: Vec<Result<T, CoolError>>) -> Self {
        let total = per_item.len();
        let mut ok = 0usize;
        let mut err = 0usize;
        let results = per_item
            .into_iter()
            .enumerate()
            .map(|(index, outcome)| match outcome {
                Ok(value) => {
                    ok += 1;
                    BatchItemResult {
                        index,
                        status: BatchItemStatus::Ok { value },
                    }
                }
                Err(error) => {
                    err += 1;
                    BatchItemResult {
                        index,
                        status: BatchItemStatus::Error {
                            error: BatchItemError::from_cool(&error),
                        },
                    }
                }
            })
            .collect();
        Self {
            results,
            summary: BatchSummary { total, ok, err },
        }
    }
}

/// Wire envelope for `POST /<model>/batch-*` request bodies. Holds
/// the items in a single field so the envelope can grow (e.g. a
/// future `client_request_id`) without breaking deserialization.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BatchRequest<I> {
    pub items: Vec<I>,
}

/// Default upper bound on items in a single batch request. Server
/// backends enforce this before any SQL runs and surface
/// [`CoolError::Validation`] on the outer `Result` when exceeded.
/// The cap is identical for all five batch operations; deviating
/// per-op would invite footguns where `batch_get` accepts a list
/// that `batch_create` of the same length rejects.
pub const BATCH_MAX_ITEMS: usize = 1000;

/// Detect duplicate keys in a batch input, loud-failing the whole
/// request when found. Returns the first duplicate (by position) so
/// the surfaced error can name a specific offending index. Linear-
/// time, allocation-only in proportion to the input length.
///
/// Used by all five batch primitives — `batch_get`, `batch_delete`,
/// `batch_update`, `batch_create` (when the input carries a client-
/// supplied PK), and `batch_upsert`. The dedup posture is deliberate:
/// silently collapsing duplicates would break the per-item `index`
/// mapping the envelope promises and hide caller bugs.
pub fn find_duplicate_position<K: Eq + std::hash::Hash>(
    keys: impl IntoIterator<Item = K>,
) -> Option<(usize, usize)> {
    let mut seen: std::collections::HashMap<K, usize> = std::collections::HashMap::new();
    for (index, key) in keys.into_iter().enumerate() {
        if let Some(&first) = seen.get(&key) {
            return Some((first, index));
        }
        seen.insert(key, index);
    }
    None
}
