//! Enum-related IR ops.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateEnum {
    /// PascalCase `.cstack` name. The Postgres emitter snake-cases
    /// this for the SQL type identifier.
    pub name: String,
    pub variants: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlterEnumAddVariant {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DropEnum {
    pub name: String,
}
