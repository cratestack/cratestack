//! Generic paginated-page envelope used by every `list` route. The shape
//! mirrors what generated clients consume.

use serde::{Deserialize, Serialize};

/// Hard ceiling on the `limit` query parameter (REST) / RPC list-input
/// field every generated list route accepts, regardless of whether the
/// model is `@@paged`. Requests above this are rejected with a `400`,
/// the same way negative `limit`/`offset` already are — see
/// `handle_list_<plural>_dispatch` in the generated code, shared
/// byte-for-byte between REST and RPC dispatch.
///
/// Without this, a caller can request an arbitrarily large `limit` and
/// force the generated handler to fetch (and, for `@@paged` models,
/// separately COUNT) an unbounded number of rows in one request — a
/// resource-exhaustion vector with no framework-level mitigation.
/// Chosen as a generous-but-real ceiling rather than a small one: it
/// should never trip on realistic paginated-UI or batch-export usage,
/// only on pathological/abusive requests.
pub const MAX_LIST_LIMIT: i64 = 1000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PageInfo {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub has_next_page: bool,
    pub has_previous_page: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page<T> {
    pub items: Vec<T>,
    pub total_count: Option<i64>,
    pub page_info: PageInfo,
}

impl<T> Page<T> {
    pub fn new(items: Vec<T>, page_info: PageInfo) -> Self {
        Self {
            items,
            total_count: None,
            page_info,
        }
    }

    pub fn with_total_count(mut self, total_count: Option<i64>) -> Self {
        self.total_count = total_count;
        self
    }
}

/// Built-in pagination-input argument type (`PageInput` in `.cstack`),
/// currently valid only as a procedure argument — the request-side mirror
/// of [`Page`]/[`PageInfo`] on the response side. Field names and
/// optionality match `PageInfo`'s own `limit`/`offset` exactly, so a
/// generated `list` route and a hand-written `PageInput`-accepting
/// procedure decode the same wire shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PageInput {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

impl PageInput {
    /// Resolves `limit`/`offset` into concrete, safe values: `limit`
    /// defaults to `max_limit` when unset and is clamped to `[0,
    /// max_limit]`; `offset` defaults to `0` and is clamped to `>= 0`.
    /// Mirrors the same rule generated `list` routes already apply to
    /// their own `limit`/`offset` input — see [`MAX_LIST_LIMIT`] — so a
    /// procedure using `PageInput` gets the identical resource-exhaustion
    /// guard for free instead of reimplementing it by hand.
    pub fn resolve(&self, max_limit: i64) -> (i64, i64) {
        let limit = self.limit.unwrap_or(max_limit).clamp(0, max_limit);
        let offset = self.offset.unwrap_or(0).max(0);
        (limit, offset)
    }
}
