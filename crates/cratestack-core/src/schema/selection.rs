//! Field selection / include shape used by the column-projection path.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectionQuery {
    pub fields: Vec<String>,
    pub includes: Vec<String>,
    pub include_fields: BTreeMap<String, Vec<String>>,
}

impl SelectionQuery {
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty() && self.includes.is_empty() && self.include_fields.is_empty()
    }
}
