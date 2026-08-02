//! Foreign-key IR, promoted from a `@relation(fields:[...],
//! references:[...])` field. Only the single-column form is
//! represented — `cratestack-parser` rejects any `@relation` that
//! doesn't declare exactly one local field and one reference.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddForeignKey {
    pub name: String,
    pub table: String,
    pub column: String,
    pub referenced_table: String,
    pub referenced_column: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DropForeignKey {
    pub name: String,
    pub table: String,
    /// The definition being dropped, carried so down-emission can
    /// reconstruct the `ADD CONSTRAINT` that reverses it — mirrors
    /// `DropCheck::kind`.
    pub column: String,
    pub referenced_table: String,
    pub referenced_column: String,
}
