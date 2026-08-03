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
//! * Index and foreign-key changes follow the same drop/add pattern in
//!   [`indexes::diff_indexes`] and [`foreign_keys::diff_foreign_keys`].
//!   A foreign key is promoted from the owning side of a `@relation`
//!   field (see `convert::relations`).
//!
//! Ops are emitted in an order that respects DDL dependencies:
//! drops first (with dependent index/FK drops before column/table
//! drops — table drops are themselves topologically sorted so a
//! table referencing another table in the same drop set drops
//! first), then creates, then index and foreign-key adds (after the
//! columns and tables they depend on exist).

mod checks;
mod columns;
mod foreign_keys;
mod indexes;
mod tables;
// `pub(crate)` (not private) so `crate::projection` — the public
// `Schema → Projections` seam — can reach `views::{ViewProjection,
// project_views}` without living inside this module.
pub(crate) mod views;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use cratestack_core::Schema;

use crate::convert::TableProjection;
use crate::ir::Op;
use crate::projection::{Projections, project};

/// Compute the migration that turns `prev` into `next`.
///
/// Thin wrapper around [`diff_projections`]: projects both schemas
/// into their [`Projections`] IR shape (see `crate::projection`) and
/// hands them to the comparison engine. Kept for existing callers
/// that only ever have two full `Schema`s on hand (e.g.
/// `cratestack-cli`'s `migrate diff`, which reads one from a snapshot
/// file and parses the other from a `.cstack` file).
pub fn diff(prev: &Schema, next: &Schema) -> Vec<Op> {
    diff_projections(&project(prev), &project(next))
}

/// Compute the migration that turns `prev` into `next`, given their
/// already-projected [`Projections`] IR — no `Schema` involved.
///
/// This is the seam a future live-database introspector (Phase B,
/// issue #204) plugs into: anything that can produce a `Projections`
/// value — not just [`project`] reading a parsed `.cstack` `Schema` —
/// can be diffed against another `Projections` value here.
pub fn diff_projections(prev: &Projections, next: &Projections) -> Vec<Op> {
    let prev_tables = &prev.tables;
    let next_tables = &next.tables;

    let rename_map = tables::resolve_renames(prev_tables, next_tables);
    let mut rename_tables = rename_map.renames;
    let mut drop_tables_ops =
        tables::collect_drops(prev_tables, next_tables, &rename_map.renamed_from);
    let (mut create_tables, mut add_indexes, mut add_checks, mut add_foreign_keys) =
        tables::collect_creates(prev_tables, next_tables, &rename_map.renamed_from);

    let mut rename_columns = Vec::new();
    let mut drop_columns = Vec::new();
    let mut add_columns = Vec::new();
    let mut alter_columns = Vec::new();
    let mut drop_indexes_ops = Vec::new();
    let mut drop_checks_ops = Vec::new();
    let mut drop_foreign_keys_ops = Vec::new();

    for (name, prev_projection) in prev_tables {
        let Some(next_projection) = find_next(name, next_tables, &rename_map.renamed_from) else {
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

        let mut fk_ops = foreign_keys::diff_foreign_keys(prev_projection, next_projection);
        add_foreign_keys.append(&mut fk_ops.adds);
        drop_foreign_keys_ops.append(&mut fk_ops.drops);
    }

    let mut view_diff = views::diff_views(&prev.views, &next.views);

    let mut ops = Vec::new();
    // Renames before table-level changes so subsequent ops can
    // reference the new names.
    ops.append(&mut rename_tables);
    ops.append(&mut rename_columns);
    // Drop CHECK constraints and foreign keys before drops on the
    // columns/tables they protect.
    ops.append(&mut drop_checks_ops);
    ops.append(&mut drop_foreign_keys_ops);
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
    // Add foreign keys last, after every table/column this migration
    // creates exists — a relation's referenced table may be one of
    // the tables `create_tables` just added.
    ops.append(&mut add_foreign_keys);
    // View creates land AFTER all column adds + table creates so
    // both source tables and any new columns the view body
    // references exist before the view definition is parsed.
    ops.append(&mut view_diff.creates);
    ops
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
