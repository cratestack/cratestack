//! [`prepare_upsert_insert`] — building the insert value set and the
//! conflict-key tuple, shared by both upsert flavors. Split out of
//! `upsert_exec.rs` purely to stay under this codebase's
//! ~200-LoC-per-file convention, not a behavioral boundary.

use cratestack_core::{CratestackContext, CratestackError};

use crate::query::support::{apply_create_defaults, find_column_value};
use crate::{ConflictTarget, ModelDescriptor, SqlValue, UpsertModelInput};

/// Compose the full insert value set (auth-derived defaults + the
/// seeded `@version` column) and the conflict-key tuple used to probe
/// and target the row. Shared by the DO UPDATE path
/// (`upsert_exec::run_upsert_in_tx`) and the DO NOTHING path
/// (`upsert_do_nothing_exec::run_upsert_do_nothing_in_tx`) — both need
/// to resolve "what row does this conflict against" identically; only
/// what happens once a conflict is found is allowed to differ between
/// them.
pub(super) fn prepare_upsert_insert<M, PK, I>(
    descriptor: &'static ModelDescriptor<M, PK>,
    input: &I,
    ctx: &CratestackContext,
    conflict_target: ConflictTarget,
) -> Result<(Vec<crate::SqlColumnValue>, Vec<(&'static str, SqlValue)>), CratestackError>
where
    I: UpsertModelInput<M>,
{
    // Reject a predicate paired with `ConflictTarget::PRIMARY_KEY`
    // before any SQL is built (cratestack#741) — the PK index is never
    // partial, so that combination can never correspond to a real
    // index. Both upsert flavors (`run_upsert_in_tx` and
    // `run_upsert_do_nothing_in_tx`) call this function first, so this
    // one check covers both.
    conflict_target.validate()?;

    // Mirrors `create_record_with_executor` so insert-branch semantics
    // stay identical to `.create()`.
    let mut insert_values =
        apply_create_defaults(input.sql_values(), descriptor.create_defaults, ctx)?;
    if let Some(version_col) = descriptor.version_column
        && find_column_value(&insert_values, version_col).is_none()
    {
        insert_values.push(crate::SqlColumnValue {
            column: version_col,
            value: crate::SqlValue::Int(0),
        });
    }
    if insert_values.is_empty() {
        return Err(CratestackError::Validation(
            "upsert input must contain at least one column".to_owned(),
        ));
    }

    // Build the conflict-key tuple by looking up each named column's
    // value in the (defaulted) insert set. The PrimaryKey branch keeps
    // the old single-column path so we don't pay an extra lookup on
    // the common case.
    //
    // A column that IS present in the insert set but whose value is a
    // `SqlValue::Null*` variant satisfies this lookup — it is not
    // treated the same as a missing column. That is deliberate: the
    // probe then binds `column = NULL`, which never matches any row
    // (three-valued SQL logic), so the upsert always takes the insert
    // branch for a NULL natural key. That is exactly the behavior a
    // `WHERE col IS NOT NULL` partial index needs (a NULL key is
    // outside the index's uniqueness domain), so this crate leans on
    // it deliberately rather than special-casing NULL conflict-column
    // values — see `ConflictTarget`'s doc comment for the same point.
    let pk_value = input.primary_key_value();
    let conflict_columns: Vec<(&'static str, SqlValue)> = match conflict_target.as_columns() {
        None => vec![(descriptor.primary_key, pk_value)],
        Some(cols) => {
            let mut out = Vec::with_capacity(cols.len());
            for col in cols {
                let value = find_column_value(&insert_values, col).cloned().ok_or_else(|| {
                    CratestackError::Validation(format!(
                        "upsert on_conflict references column `{col}` which is not present in the input",
                    ))
                })?;
                out.push((*col, value));
            }
            out
        }
    };

    Ok((insert_values, conflict_columns))
}
