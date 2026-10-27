//! The SQLite `DEFAULT` expressions for cratestack's own `@default`
//! functions, shared by the migration emitter's SQLite tree and the
//! embedded runtime's `create_table_sql` (cratestack#1234), so the two
//! write the same thing.
//!
//! Each produces exactly what the embedded runtime binds and decodes: RFC
//! 3339 UTC text for `now()`, and a hyphenated lowercase v4 UUID for
//! `uuid()`. A function call is only accepted in a SQLite `DEFAULT` inside
//! parentheses, so both are wrapped. `cuid()` has no SQLite equivalent.

use super::default_functions::BuiltinDefault;

const NOW: &str = "(strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))";

/// A random v4 UUID: 122 random bits, the version nibble `4`, and the
/// variant nibble drawn from `8`, `9`, `a`, `b`.
const UUID_V4: &str = "(lower(hex(randomblob(4))) || '-' || lower(hex(randomblob(2))) \
                       || '-4' || substr(lower(hex(randomblob(2))), 2) || '-' \
                       || substr('89ab', 1 + (abs(random()) % 4), 1) \
                       || substr(lower(hex(randomblob(2))), 2) || '-' \
                       || lower(hex(randomblob(6))))";

impl BuiltinDefault {
    /// The SQLite `DEFAULT` expression, or `None` for `cuid()`.
    pub fn sqlite_default(self) -> Option<&'static str> {
        match self {
            Self::Now => Some(NOW),
            Self::Uuid => Some(UUID_V4),
            Self::Cuid => None,
        }
    }
}
