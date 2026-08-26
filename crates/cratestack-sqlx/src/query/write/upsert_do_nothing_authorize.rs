//! [`authorize_existing_row`] — the update-policy re-check
//! `run_upsert_do_nothing_in_tx` runs on its `Existing` branches. Split
//! out purely to stay under this codebase's ~200-LoC-per-file
//! convention, not a behavioral boundary.

use cratestack_core::{CratestackContext, CratestackError};

use crate::{ModelDescriptor, SqlValue, SqlxRuntime};

use super::upsert_sql::row_passes_update_policy;

/// Mirrors `upsert_exec::run_upsert_in_tx`'s "both create AND update
/// policy must allow" invariant. `.do_nothing()` never mutates an
/// existing row, but it does hand the caller that row's current
/// contents — skipping this check would let a caller who only has
/// create authorization probe for a row's existence/contents through
/// this call site, which is exactly the leak the DO UPDATE path's
/// identical check exists to close off. Not a change to
/// policy-evaluation logic: this calls the same
/// `row_passes_update_policy` the DO UPDATE path already used, just
/// from the DO NOTHING execution path.
pub(super) async fn authorize_existing_row<M, PK>(
    runtime: &SqlxRuntime,
    descriptor: &'static ModelDescriptor<M, PK>,
    conflict_columns: &[(&'static str, SqlValue)],
    predicate: Option<&'static str>,
    ctx: &CratestackContext,
) -> Result<(), CratestackError> {
    if !row_passes_update_policy(runtime.pool(), descriptor, conflict_columns, predicate, ctx)
        .await?
    {
        return Err(CratestackError::Forbidden(
            "update policy denied this upsert".to_owned(),
        ));
    }
    Ok(())
}
