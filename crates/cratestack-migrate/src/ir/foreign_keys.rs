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
    pub on_delete: ForeignKeyAction,
    pub on_update: ForeignKeyAction,
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
    pub on_delete: ForeignKeyAction,
    pub on_update: ForeignKeyAction,
}

/// `ON DELETE`/`ON UPDATE` referential action, from `@relation(...,
/// onDelete: ..., onUpdate: ...)`. `NoAction` is both the SQL standard
/// default and what every relation got implicitly before this
/// attribute existed, so it's the default here too — an emitter omits
/// the clause entirely for `NoAction` rather than spelling out
/// Postgres's own default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ForeignKeyAction {
    Cascade,
    Restrict,
    SetNull,
    SetDefault,
    #[default]
    NoAction,
}

impl ForeignKeyAction {
    /// Parses the bareword `cratestack-parser` already validated
    /// (`onDelete: Cascade`, not `onDelete: "Cascade"`). Schemas
    /// reaching `cratestack-migrate` have already passed `check`, so
    /// an unrecognised word shouldn't occur here — the fallback is
    /// `NoAction` (not a panic) since this is defense in depth, not
    /// an invariant this crate itself establishes.
    pub(crate) fn parse(value: &str) -> Self {
        match value {
            "Cascade" => Self::Cascade,
            "Restrict" => Self::Restrict,
            "SetNull" => Self::SetNull,
            "SetDefault" => Self::SetDefault,
            _ => Self::NoAction,
        }
    }
}
