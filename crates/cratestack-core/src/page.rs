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
