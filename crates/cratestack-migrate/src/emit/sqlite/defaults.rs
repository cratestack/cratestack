//! How a column default renders on SQLite.
//!
//! SQLite accepts a function call in `DEFAULT` only inside parentheses, so
//! every non-literal default is wrapped. cratestack's own `now()` and
//! `uuid()` become the expressions in
//! `cratestack_core::schema::default_functions::BuiltinDefault::sqlite_default`,
//! which the embedded runtime's `create_table_sql` writes too.

use std::borrow::Cow;

use cratestack_core::schema::default_functions::BuiltinDefault;

use crate::ir::ColumnDefault;

/// The `DEFAULT` expression, or `None` when the column gets no database
/// default: a `dbgenerated()` marker, or `cuid()`, which cratestack alone
/// generates.
pub(super) fn render_default(default: &ColumnDefault) -> Option<Cow<'_, str>> {
    match default {
        ColumnDefault::Literal(value) => Some(value.into()),
        ColumnDefault::Function(call) => Some(format!("({call})").into()),
        ColumnDefault::Generated(generated) => BuiltinDefault::from(*generated)
            .sqlite_default()
            .map(Cow::Borrowed),
        ColumnDefault::DbGenerated => None,
    }
}
