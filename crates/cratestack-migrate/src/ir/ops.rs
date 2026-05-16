//! Op-payload structs for table/column/index/rename operations.
//! Enum-related ops live in [`super::enums`]; check-constraint ops in
//! [`super::checks`].

use serde::{Deserialize, Serialize};

use super::columns::{Column, ColumnArity, ColumnDefault, ColumnType};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateTable {
    pub name: String,
    pub columns: Vec<Column>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DropTable {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddColumn {
    pub table: String,
    pub column: Column,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DropColumn {
    pub table: String,
    pub column: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddIndex {
    pub name: String,
    pub table: String,
    pub columns: Vec<String>,
    pub unique: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DropIndex {
    pub name: String,
    pub table: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlterColumnType {
    pub table: String,
    pub column: String,
    pub from: ColumnType,
    pub from_arity: ColumnArity,
    pub to: ColumnType,
    pub to_arity: ColumnArity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlterColumnNullability {
    pub table: String,
    pub column: String,
    pub from: ColumnArity,
    pub to: ColumnArity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlterColumnDefault {
    pub table: String,
    pub column: String,
    pub from: Option<ColumnDefault>,
    pub to: Option<ColumnDefault>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenameTable {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenameColumn {
    pub table: String,
    pub from: String,
    pub to: String,
}
