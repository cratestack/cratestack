//! The `up.pre.sql` scaffold, and the `up.sql` warning that points at it.
//!
//! Both strings are generated from the same
//! [`BlockingReason`](crate::ir::BlockingReason) list, so the warning
//! can never describe a different operation than the one the scaffold
//! offers to fix — which is how cratestack#843 happened. The warning
//! named `up.pre.sql` as the remedy while nothing wrote or read such a
//! file, and separately described the blocking op as "a required column
//! was added without a default" even when it was an `Optional →
//! Required` promotion.

use std::fmt::Write as _;

use crate::ir::BlockingReason;

/// The generated `up.pre.sql` body: a comment-only template naming each
/// blocking op and, where one exists, the shape of the SQL that
/// unblocks it.
///
/// Comment-only by design. `cratestack-service`'s loader treats a file
/// with no executable statement as absent, so a scaffold the operator
/// never fills in costs nothing: no round-trip, and no change to the
/// migration's checksum.
pub(super) fn scaffold(reasons: &[BlockingReason]) -> String {
    let mut sql = String::new();
    sql.push_str("-- up.pre.sql — preparatory SQL for the migration in this directory.\n");
    sql.push_str("--\n");
    sql.push_str("-- The runner executes this file immediately before `up.sql`, inside\n");
    sql.push_str("-- the SAME transaction, and folds it into the migration checksum.\n");
    sql.push_str("-- Both halves land or neither does.\n");
    sql.push_str("--\n");
    sql.push_str("-- cratestack generated this file because `up.sql` contains a blocking\n");
    sql.push_str("-- operation: it cannot succeed against a table that already has rows\n");
    sql.push_str("-- until the existing data is prepared first. On an empty table — a\n");
    sql.push_str("-- fresh CI database, say — it will pass with this file left as-is,\n");
    sql.push_str("-- which is exactly why the problem tends to surface first in\n");
    sql.push_str("-- production. Fill it in before deploying anywhere with data.\n");
    sql.push_str("--\n");
    sql.push_str("-- This file is yours to edit; `up.sql` is generated. Leaving it as\n");
    sql.push_str("-- comments only is a valid choice — it is then treated as absent.\n");
    sql.push_str("--\n");
    push_reasons(&mut sql, reasons);
    sql
}

/// The header prepended to `up.sql` when the migration blocks. Names
/// the real mechanism and each real operation.
pub(super) fn up_warning(reasons: &[BlockingReason]) -> String {
    let mut sql = String::new();
    sql.push_str("-- WARNING: this migration contains blocking operations. It cannot\n");
    sql.push_str("-- succeed against a table that already has rows until the existing\n");
    sql.push_str("-- data is prepared first:\n");
    sql.push_str("--\n");
    push_reasons(&mut sql, reasons);
    sql.push_str("--\n");
    sql.push_str("-- Put that preparation in `up.pre.sql`, alongside this file — it has\n");
    sql.push_str("-- been scaffolded for you. It runs immediately before this file, in\n");
    sql.push_str("-- the same transaction, and is checksummed with it.\n");
    sql.push('\n');
    sql
}

/// The shared body: one bullet per blocking op, each followed by its
/// remedy template where one exists.
fn push_reasons(sql: &mut String, reasons: &[BlockingReason]) {
    for reason in reasons {
        writeln!(sql, "--   - {}: {}", reason.target(), reason.cause).ok();
        match &reason.remedy {
            Some(remedy) => {
                writeln!(sql, "--     {remedy}").ok();
            }
            // No template rather than a wrong one: a column that does
            // not exist yet cannot be backfilled by a statement that
            // runs before it is added.
            None => {
                sql.push_str(
                    "--     No pre-script can fix this one: the column does not exist\n\
                     --     yet when this file runs. Either give it a default in the\n\
                     --     schema, or split the change into two migrations — add it\n\
                     --     optional, backfill, then promote it to required.\n",
                );
            }
        }
    }
}

#[cfg(test)]
mod tests;
