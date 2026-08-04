//! Model-level `@@unique([...])` → composite `CREATE UNIQUE INDEX`.
//!
//! Field-level `@unique` is projected in [`super::project_model`] as a
//! single-column index; this is the multi-column counterpart. Both land
//! as the same [`AddIndex`] op, so the emitters need no composite-aware
//! code path — they already join `columns` into the index DDL.

use std::collections::HashSet;

use cratestack_core::{Model, parse_composite_unique_attribute};

use crate::ir::{AddIndex, Column};
use crate::naming::{column_name, index_name_unique};

/// One `AddIndex` per `@@unique([...])` on the model, in declaration
/// order.
///
/// Malformed attributes and names that don't resolve to a projected
/// column are skipped rather than surfaced as errors: `cratestack
/// check` is the gate for those (see
/// `cratestack_parser::validate::composite_attributes`), and this
/// function also runs over the *previous* schema read back from
/// `schema.snapshot.json`, which may have been written by an older
/// toolchain. Emitting a half-resolved index there would produce DDL
/// referencing a column that does not exist.
pub(super) fn composite_unique_indexes(
    model: &Model,
    table: &str,
    columns: &[Column],
) -> Vec<AddIndex> {
    let projected: HashSet<&str> = columns.iter().map(|c| c.name.as_str()).collect();

    let mut indexes = Vec::new();
    for attribute in &model.attributes {
        if !attribute.raw.starts_with("@@unique(") {
            continue;
        }
        let Ok(fields) = parse_composite_unique_attribute(&attribute.raw) else {
            continue;
        };
        let index_columns: Vec<String> = fields.iter().map(|field| column_name(field)).collect();
        if !index_columns
            .iter()
            .all(|name| projected.contains(name.as_str()))
        {
            continue;
        }
        let borrowed: Vec<&str> = index_columns.iter().map(String::as_str).collect();
        indexes.push(AddIndex {
            name: index_name_unique(table, &borrowed),
            table: table.to_owned(),
            columns: index_columns,
            unique: true,
            using: None,
            opclass: None,
        });
    }
    indexes
}
