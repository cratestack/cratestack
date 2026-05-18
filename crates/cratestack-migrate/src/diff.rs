//! Compute a list of [`Op`]s that turns one [`Schema`] into another.
//!
//! The algorithm is deliberately conservative:
//!
//! * Tables and columns are matched **by name only**. Renames are not
//!   inferred from text — they must be declared via `@rename` (slice
//!   9). A column that disappears and a new column that appears look
//!   exactly the same here, and the engine treats them as drop + add.
//! * Column *changes* (type, nullability, default) are detected in
//!   [`columns::diff_columns`].
//! * Index changes follow the same drop/add pattern in
//!   [`indexes::diff_indexes`].
//!
//! Ops are emitted in an order that respects DDL dependencies:
//! drops first (with dependent index drops before column/table drops),
//! then creates, then index adds (after the columns that back them
//! exist).

mod checks;
mod columns;
mod enums;
mod indexes;
mod tables;
mod views;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use cratestack_core::Schema;

use crate::convert::{TableProjection, project_model};
use crate::ir::Op;

/// Compute the migration that turns `prev` into `next`.
pub fn diff(prev: &Schema, next: &Schema) -> Vec<Op> {
    let prev_tables = project_tables(prev);
    let next_tables = project_tables(next);

    let (mut create_enums, mut alter_enums, mut drop_enums) = enums::diff_enums(prev, next);
    let rename_map = tables::resolve_renames(&prev_tables, &next_tables);
    let mut rename_tables = rename_map.renames;
    let mut drop_tables_ops =
        tables::collect_drops(&prev_tables, &next_tables, &rename_map.renamed_from);
    let (mut create_tables, mut add_indexes, mut add_checks) =
        tables::collect_creates(&prev_tables, &next_tables, &rename_map.renamed_from);

    let mut rename_columns = Vec::new();
    let mut drop_columns = Vec::new();
    let mut add_columns = Vec::new();
    let mut alter_columns = Vec::new();
    let mut drop_indexes_ops = Vec::new();
    let mut drop_checks_ops = Vec::new();

    for (name, prev_projection) in &prev_tables {
        let Some(next_projection) = find_next(name, &next_tables, &rename_map.renamed_from) else {
            continue;
        };

        let mut col_ops = columns::diff_columns(prev_projection, next_projection);
        rename_columns.append(&mut col_ops.renames);
        drop_columns.append(&mut col_ops.drops);
        add_columns.append(&mut col_ops.adds);
        alter_columns.append(&mut col_ops.alters);

        let mut check_ops = checks::diff_checks(prev_projection, next_projection);
        add_checks.append(&mut check_ops.adds);
        drop_checks_ops.append(&mut check_ops.drops);

        let mut idx_ops = indexes::diff_indexes(prev_projection, next_projection);
        add_indexes.append(&mut idx_ops.adds);
        drop_indexes_ops.append(&mut idx_ops.drops);
    }

    let mut view_diff = views::diff_views(prev, next);

    let mut ops = Vec::new();
    // Enum creates first so tables can reference them.
    ops.append(&mut create_enums);
    ops.append(&mut alter_enums);
    // Renames before table-level changes so subsequent ops can
    // reference the new names.
    ops.append(&mut rename_tables);
    ops.append(&mut rename_columns);
    // Drop CHECK constraints before drops on the columns they protect.
    ops.append(&mut drop_checks_ops);
    ops.append(&mut drop_indexes_ops);
    // View drops land BEFORE column drops and table drops (ADR-0003
    // §"Migration emission"). Postgres rejects a `DROP COLUMN` /
    // `DROP TABLE` while a dependent view still references it, so any
    // view that touches a soon-to-be-dropped column/table has to be
    // gone first. Body changes are also modelled as drop + create
    // (see `diff/views.rs::ViewDiff`), so this is also the position
    // where the "old body" of a view-body-change disappears before
    // its referenced columns can be dropped.
    ops.append(&mut view_diff.drops);
    ops.append(&mut drop_columns);
    ops.append(&mut drop_tables_ops);
    ops.append(&mut create_tables);
    ops.append(&mut add_columns);
    ops.append(&mut alter_columns);
    ops.append(&mut add_indexes);
    // Add CHECK constraints after the columns they protect exist.
    ops.append(&mut add_checks);
    // View creates land AFTER all column adds + table creates so
    // both source tables and any new columns the view body
    // references exist before the view definition is parsed.
    ops.append(&mut view_diff.creates);
    // Enum drops last — after any tables that depended on them.
    ops.append(&mut drop_enums);
    ops
}

fn project_tables(schema: &Schema) -> BTreeMap<String, TableProjection> {
    schema
        .models
        .iter()
        .map(|model| {
            let projection = project_model(model, schema);
            (projection.name.clone(), projection)
        })
        .collect()
}

/// Find the projection on the next side for a prev-side table name,
/// honoring rename markers when the direct lookup misses.
fn find_next<'a>(
    name: &str,
    next_tables: &'a BTreeMap<String, TableProjection>,
    renamed_from: &BTreeMap<&str, &str>,
) -> Option<&'a TableProjection> {
    if let Some(projection) = next_tables.get(name) {
        return Some(projection);
    }
    let renamed_new = renamed_from
        .iter()
        .find_map(|(new, old)| (*old == name).then_some(*new));
    renamed_new.and_then(|new| next_tables.get(new))
}
