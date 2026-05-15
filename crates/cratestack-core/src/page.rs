//! Generic paginated-page envelope used by every `list` route. The shape
//! mirrors what generated clients consume.

use serde::{Deserialize, Serialize};

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
