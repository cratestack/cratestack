use super::ModelDescriptor;
use crate::{SqlColumnValue, SqlValue};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateDefaultType {
    Bool,
    Int,
    BigInt,
    String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CreateDefault {
    pub column: &'static str,
    pub auth_field: &'static str,
    pub ty: CreateDefaultType,
    pub nullable: bool,
    /// Whether the auth field is required (non-optional) in the auth block.
    /// When true, a missing auth field should cause validation to fail,
    /// even if the model field is nullable. This prevents tenant-isolation
    /// issues where NULL values bypass policy predicates.
    pub auth_field_required: bool,
}

/// What cratestack generates for a `@default(now())`, `@default(uuid())`
/// or `@default(cuid())` column (cratestack#1209). Chosen per field type at
/// codegen, so the value binds as the column's own type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeneratedValue {
    /// `now()` on a `DateTime` field.
    Now,
    /// `uuid()` on a `Uuid` field.
    Uuid,
    /// `uuid()` on a `String` field: the hyphenated text form.
    UuidText,
    /// `cuid()` on a `Cuid` or `String` field.
    Cuid,
}

impl GeneratedValue {
    pub fn generate(self) -> SqlValue {
        match self {
            Self::Now => SqlValue::DateTime(chrono::Utc::now()),
            Self::Uuid => SqlValue::Uuid(uuid::Uuid::new_v4()),
            Self::UuidText => SqlValue::String(uuid::Uuid::new_v4().hyphenated().to_string()),
            Self::Cuid => SqlValue::String(cratestack_core::cuid::new_cuid()),
        }
    }
}

/// A column cratestack fills on create when the input leaves it out.
///
/// Generated in Rust rather than left to a database `DEFAULT`, so the
/// value is the same on every backend: an embedded runtime's table may
/// predate its `DEFAULT` clauses (cratestack#1234), and `cuid()` has no
/// database equivalent at all.
/// The migration emitter still writes a matching database default where
/// one exists, for writes that do not go through cratestack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GeneratedDefault {
    pub column: &'static str,
    pub value: GeneratedValue,
}

impl<M, PK> ModelDescriptor<M, PK> {
    /// Attach the model's generated defaults. A separate step from
    /// [`ModelDescriptor::new`] so descriptors built without one keep
    /// compiling unchanged.
    pub const fn with_generated_defaults(
        self,
        generated_defaults: &'static [GeneratedDefault],
    ) -> Self {
        Self {
            generated_defaults,
            ..self
        }
    }

    /// Append a generated value for each generated-default column the
    /// create values do not already carry.
    pub fn fill_generated_defaults(&self, values: &mut Vec<SqlColumnValue>) {
        for default in self.generated_defaults {
            if values.iter().all(|value| value.column != default.column) {
                values.push(SqlColumnValue {
                    column: default.column,
                    value: default.value.generate(),
                });
            }
        }
    }
}
