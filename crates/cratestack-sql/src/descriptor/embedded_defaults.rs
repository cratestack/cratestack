//! Defaults the embedded runtime applies itself (cratestack#1234).
//!
//! The server runtime leaves a literal `@default` to the column's `DEFAULT`
//! clause, which the migration writes. The embedded runtime's tables are
//! created by `cratestack_rusqlite::ddl::create_table_sql` instead, and
//! `CREATE TABLE IF NOT EXISTS` never alters a table an older version
//! created, so it binds the literal on create, and writes the clause for
//! new tables from the same value.

use cratestack_core::schema::default_functions::BuiltinDefault;

use super::ModelDescriptor;
use super::defaults::GeneratedValue;
use crate::{SqlColumnValue, SqlValue};

/// A column with a literal `@default`. `value` builds the literal's
/// `SqlValue`, the one the create input would bind for it.
#[derive(Clone, Copy)]
pub struct LiteralDefault {
    pub column: &'static str,
    pub value: fn() -> SqlValue,
}

impl std::fmt::Debug for LiteralDefault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiteralDefault")
            .field("column", &self.column)
            .field("value", &(self.value)())
            .finish()
    }
}

/// A required column whose `@default` only the database can fill:
/// `dbgenerated()` or a registered function, spelled as `default`. On the
/// embedded runtime nothing fills it unless the app adds a trigger or a
/// `DEFAULT`, so a create that leaves it NULL is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DatabaseDefault {
    pub column: &'static str,
    pub default: &'static str,
}

impl GeneratedValue {
    /// The cratestack function this value comes from.
    pub fn builtin(self) -> BuiltinDefault {
        match self {
            Self::Now => BuiltinDefault::Now,
            Self::Uuid | Self::UuidText => BuiltinDefault::Uuid,
            Self::Cuid => BuiltinDefault::Cuid,
        }
    }
}

impl<M, PK> ModelDescriptor<M, PK> {
    /// Attach the model's literal and database-filled defaults.
    pub const fn with_embedded_defaults(
        self,
        literal_defaults: &'static [LiteralDefault],
        database_defaults: &'static [DatabaseDefault],
    ) -> Self {
        Self {
            literal_defaults,
            database_defaults,
            ..self
        }
    }

    /// Append the literal for each literal-default column the create
    /// values do not already carry.
    pub fn fill_literal_defaults(&self, values: &mut Vec<SqlColumnValue>) {
        for default in self.literal_defaults {
            if values.iter().all(|value| value.column != default.column) {
                values.push(SqlColumnValue {
                    column: default.column,
                    value: (default.value)(),
                });
            }
        }
    }
}
