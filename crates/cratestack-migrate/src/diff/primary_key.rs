//! Detects a change to an *existing* table's primary key (issue #536).
//!
//! Every other phase in this module (`columns`, `indexes`, `checks`,
//! `foreign_keys`) turns a detected difference into one or more
//! [`crate::ir::Op`]s. This one deliberately does not: a correct
//! primary-key migration needs constraint drop/recreate ordering,
//! dependent foreign keys, and a data-safety story for a populated
//! table (dropping and re-adding a `PRIMARY KEY` on a table with rows
//! is a genuinely dangerous operation) — none of which the IR or the
//! emitters model today. `@@id([...])` is also currently unreachable
//! through `include_*_schema!` at all (rejected at macro expansion,
//! issue #136), so there is no live caller depending on a half-built
//! DDL path for it.
//!
//! The previous behavior — silently producing an empty diff — is
//! strictly worse than refusing: it lets the schema and the database
//! drift apart with no diff, no error, and no warning. So this phase
//! fails loudly instead, naming the table and the before/after key,
//! and leaves writing the actual migration to a human.

use std::collections::BTreeMap;

use super::columns::column_rename_map;
use crate::convert::TableProjection;
use crate::error::MigrateError;

/// A table's primary-key column list, in the same order
/// `emit::postgres::tables` / `emit::sqlite::tables` already use to
/// render `PRIMARY KEY (...)`: every `primary_key`-flagged column, in
/// column-declaration order.
///
/// This is *not* the same as `@@id([...])`'s literal argument order —
/// `convert::project_model` already collapses that list into a
/// `HashSet` before it reaches a [`TableProjection`], so argument
/// order alone carries no information here. That is a separate,
/// pre-existing gap in order fidelity (`@@id`'s argument order is
/// discarded, matching a like quirk in DDL emission itself) and is
/// out of scope for this fix — this function only needs to agree with
/// what the emitters would actually produce, so a "primary key
/// changed" refusal here always corresponds to a real difference in
/// emitted DDL.
fn primary_key_columns(table: &TableProjection) -> Vec<&str> {
    table
        .columns
        .iter()
        .filter(|column| column.primary_key)
        .map(|column| column.name.as_str())
        .collect()
}

/// Refuses a diff between `prev` and `next` (the same logical table,
/// matched by name/rename — see `tables::resolve_renames`) whose
/// primary-key column set or order differs. A no-op for a brand-new
/// table: `tables::collect_creates` never calls this, since a
/// `CreateTable` sets `primary_key` directly and there is nothing to
/// "change".
///
/// A column-level `@rename(from = "...")` on a key column is not a
/// key change: `columns::diff_columns` already turns it into a plain
/// `RenameColumn` op, and `RENAME COLUMN` preserves whatever
/// constraint (including `PRIMARY KEY`) already references the
/// column on both backends. So before comparing, every prev-side key
/// column name is resolved through the same rename map
/// `columns::diff_columns` uses — a key column that was renamed
/// compares by its *next*-side name, in its original position. A
/// renamed key column that also changed position, or a genuine
/// change to the key's column set, still compares unequal and is
/// still refused (see `diff/tests/primary_key.rs`'s
/// `reordering_primary_key_field_declarations_is_rejected` and
/// `changing_composite_primary_key_columns_is_rejected`).
pub(super) fn check_primary_key_unchanged(
    prev: &TableProjection,
    next: &TableProjection,
) -> Result<(), MigrateError> {
    let prev_key = primary_key_columns(prev);
    let next_key = primary_key_columns(next);

    let renames = column_rename_map(prev, next);
    let old_to_new: BTreeMap<&str, &str> = renames.iter().map(|(new, old)| (*old, *new)).collect();
    let resolved_prev_key: Vec<&str> = prev_key
        .iter()
        .map(|name| old_to_new.get(name).copied().unwrap_or(*name))
        .collect();

    if resolved_prev_key == next_key {
        return Ok(());
    }
    Err(MigrateError::PrimaryKeyChanged {
        table: next.name.clone(),
        prev: format_key(&prev_key),
        next: format_key(&next_key),
    })
}

fn format_key(columns: &[&str]) -> String {
    if columns.is_empty() {
        "(none)".to_owned()
    } else {
        format!("({})", columns.join(", "))
    }
}
