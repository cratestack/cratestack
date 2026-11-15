//! The SQL names a model's `@@map` / `@map` markers give its table and columns
//! (cratestack#1375), read through the one reader every generator shares.

use cratestack_core::Model;

use crate::naming::column_name;

/// The column the model's field `field` is stored in. A name that is no field
/// of the model (a malformed `@@unique`, read back from an older snapshot)
/// falls back to the derived name, which the callers then skip as unprojected.
pub(super) fn column_of(model: &Model, field: &str) -> String {
    model
        .sql_column_of(field)
        .unwrap_or_else(|| column_name(field))
}
