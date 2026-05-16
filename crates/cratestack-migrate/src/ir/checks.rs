//! CHECK-constraint IR. One CHECK constraint is promoted from a
//! `.cstack` validator marked `@db_enforce`; the IR captures the
//! *kind* of validator so each emitter renders the predicate in its
//! own dialect, rather than a raw SQL fragment.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddCheck {
    pub table: String,
    pub column: String,
    pub name: String,
    pub kind: CheckKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DropCheck {
    pub table: String,
    pub column: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CheckKind {
    /// `@range(min, max)` — numeric bounds. Either bound may be absent.
    Range { min: Option<i64>, max: Option<i64> },
    /// `@length(min, max)` — string/bytes length bounds.
    Length { min: Option<i64>, max: Option<i64> },
    /// `@iso4217` — three ASCII uppercase letters.
    Iso4217,
}
