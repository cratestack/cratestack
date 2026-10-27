//! [`ColumnDefault`]: a column's default, and how a snapshot's stored
//! defaults are normalised when read back.

use cratestack_core::schema::default_functions::{BuiltinDefault, DefaultValue, classify_default};
use cratestack_core::schema::default_literal::{sql_string, unquote};
use serde::{Deserialize, Deserializer, Serialize};

/// Column default value, captured as the developer wrote it. The
/// emitter is responsible for any dialect-specific quoting.
///
/// Deserializing normalises a stored default by what its text means, so a
/// snapshot written by an older cratestack never reverses into DDL that
/// the database refuses:
///
/// - a marker (any case, spacing, argument or trailing text, or the bare
///   word) becomes [`ColumnDefault::DbGenerated`] (cratestack#1128);
/// - `now()`, `uuid()` or `cuid()`, in any spelling, becomes
///   [`ColumnDefault::Generated`] (cratestack#1209). An old `Function("uuid()")` would otherwise
///   reverse into `SET DEFAULT uuid()`, a function no database has.
///
/// An `auth().field` default never reaches DDL at all; see
/// [`deserialize_column_default`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "ColumnDefaultRepr")]
pub enum ColumnDefault {
    /// Literal (e.g. `0`, `'pending'`, `true`).
    Literal(String),
    /// A database function call (e.g. a function the datasource registers
    /// with `functions = [...]`), written verbatim.
    Function(String),
    /// `@default(dbgenerated())` — a marker, not a value. It asserts
    /// that the column already has (or will separately be given) a
    /// real Postgres-level default set some other way: hand-authored
    /// migration SQL, a trigger, `GENERATED ... AS IDENTITY`, etc.
    /// cratestack has no way to verify that claim from the `.cstack`
    /// schema alone, so emitters must never invent a `DEFAULT` clause
    /// for it — see `Op::destructiveness` and
    /// `crate::ir::unverified_dbgenerated_columns` for the
    /// corresponding safety checks.
    DbGenerated,
    /// `@default(now())`, `uuid()` or `cuid()`. cratestack generates the
    /// value on create; emitters write the database's equivalent default
    /// where it has one, for writes that bypass cratestack.
    Generated(GeneratedDefault),
}

/// One of cratestack's own `@default` functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GeneratedDefault {
    Now,
    Uuid,
    Cuid,
}

impl From<BuiltinDefault> for GeneratedDefault {
    fn from(builtin: BuiltinDefault) -> Self {
        match builtin {
            BuiltinDefault::Now => Self::Now,
            BuiltinDefault::Uuid => Self::Uuid,
            BuiltinDefault::Cuid => Self::Cuid,
        }
    }
}

impl From<GeneratedDefault> for BuiltinDefault {
    fn from(generated: GeneratedDefault) -> Self {
        match generated {
            GeneratedDefault::Now => Self::Now,
            GeneratedDefault::Uuid => Self::Uuid,
            GeneratedDefault::Cuid => Self::Cuid,
        }
    }
}

impl ColumnDefault {
    /// The default a `.cstack` `@default(<inner>)` stores in DDL, or `None`
    /// when it stores none (`auth().field` is filled from the auth context).
    pub fn from_inner(inner: &str) -> Option<Self> {
        match classify_default(inner) {
            DefaultValue::Marker(_) => Some(Self::DbGenerated),
            DefaultValue::Auth => None,
            DefaultValue::Builtin(builtin, _) => Some(Self::Generated(builtin.into())),
            DefaultValue::Function(_) => Some(Self::Function(inner.to_owned())),
            DefaultValue::Literal => Some(Self::Literal(inner.to_owned())),
        }
    }

    /// Whether the database fills existing rows with this default when the
    /// column is added. A marker backfills nothing, and `cuid()` has no
    /// database equivalent.
    pub fn backfills(&self) -> bool {
        match self {
            Self::Literal(_) | Self::Function(_) => true,
            Self::Generated(generated) => *generated != GeneratedDefault::Cuid,
            Self::DbGenerated => false,
        }
    }
}

/// [`ColumnDefault`]'s on-disk shape, before normalisation.
#[derive(Deserialize)]
enum ColumnDefaultRepr {
    Literal(String),
    Function(String),
    DbGenerated,
    Generated(GeneratedDefault),
}

impl From<ColumnDefaultRepr> for ColumnDefault {
    fn from(repr: ColumnDefaultRepr) -> Self {
        let text = match repr {
            ColumnDefaultRepr::DbGenerated => return Self::DbGenerated,
            ColumnDefaultRepr::Generated(generated) => return Self::Generated(generated),
            ColumnDefaultRepr::Literal(text) | ColumnDefaultRepr::Function(text) => text,
        };
        match classify_default(&text) {
            DefaultValue::Marker(_) => Self::DbGenerated,
            DefaultValue::Builtin(builtin, _) => Self::Generated(builtin.into()),
            DefaultValue::Function(_) => Self::Function(text),
            DefaultValue::Auth | DefaultValue::Literal => Self::Literal(stored_literal(text)),
        }
    }
}

/// A literal stored before cratestack#1234 kept a double-quoted string as
/// written, `"draft"`, which the emitter wrote as `DEFAULT "draft"`: a
/// column reference to Postgres. It reads back as the SQL string the
/// emitter now writes, so the next diff does not see a change.
fn stored_literal(text: String) -> String {
    match text.starts_with('"').then(|| unquote(&text)).flatten() {
        Some(unquoted) => sql_string(&unquoted),
        None => text,
    }
}

/// `Column::default`'s deserializer. A snapshot written before
/// cratestack#1209 stores `@default(auth().field)` as the literal
/// `auth().field`, which the emitter wrote as `DEFAULT auth().field`; it is
/// read back as no default, which is what the column has.
pub(crate) fn deserialize_column_default<'de, D>(
    deserializer: D,
) -> Result<Option<ColumnDefault>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<ColumnDefault>::deserialize(deserializer)?.filter(
        |default| !matches!(default, ColumnDefault::Literal(text) if classify_default(text) == DefaultValue::Auth),
    ))
}

#[cfg(test)]
#[path = "column_default_tests.rs"]
mod tests;
