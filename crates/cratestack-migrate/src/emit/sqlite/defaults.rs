//! How a column default renders on SQLite.
//!
//! SQLite accepts a function call in `DEFAULT` only inside parentheses, so
//! every non-literal default is wrapped. cratestack's own `now()` and
//! `uuid()` become expressions that produce exactly what the embedded
//! runtime binds and decodes: RFC 3339 UTC text, and a hyphenated
//! lowercase v4 UUID.

use std::borrow::Cow;

use crate::ir::{ColumnDefault, GeneratedDefault};

const NOW: &str = "(strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))";

/// A random v4 UUID: 122 random bits, the version nibble `4`, and the
/// variant nibble drawn from `8`, `9`, `a`, `b`.
const UUID_V4: &str = "(lower(hex(randomblob(4))) || '-' || lower(hex(randomblob(2))) \
                       || '-4' || substr(lower(hex(randomblob(2))), 2) || '-' \
                       || substr('89ab', 1 + (abs(random()) % 4), 1) \
                       || substr(lower(hex(randomblob(2))), 2) || '-' \
                       || lower(hex(randomblob(6))))";

/// The `DEFAULT` expression, or `None` when the column gets no database
/// default: a `dbgenerated()` marker, or `cuid()`, which cratestack alone
/// generates.
pub(super) fn render_default(default: &ColumnDefault) -> Option<Cow<'_, str>> {
    match default {
        ColumnDefault::Literal(value) => Some(value.into()),
        ColumnDefault::Function(call) => Some(format!("({call})").into()),
        ColumnDefault::Generated(GeneratedDefault::Now) => Some(NOW.into()),
        ColumnDefault::Generated(GeneratedDefault::Uuid) => Some(UUID_V4.into()),
        ColumnDefault::Generated(GeneratedDefault::Cuid) | ColumnDefault::DbGenerated => None,
    }
}
