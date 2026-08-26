//! SQL emitter for the `DO UPDATE` conflict path: the genuine `INSERT
//! ... ON CONFLICT (...) DO UPDATE SET ... RETURNING ...` (or its
//! no-op-self-assignment fallback for a model with nothing eligible to
//! overwrite). Split out from `upsert_sql.rs` (the pre-lock probes)
//! purely to stay under this codebase's ~200-LoC-per-file convention —
//! same module (`write`), no behavioral boundary implied by the split.

use cratestack_core::CratestackError;

use crate::query::support::{classify_unique_violation, push_bind_value};
use crate::{ConflictTarget, ModelDescriptor, SqlColumnValue, sqlx};

/// Render and execute the conflict-bearing INSERT. The DO UPDATE
/// clause references only columns in
/// `descriptor.upsert_update_columns` — PK, `@version`, `@readonly`,
/// `@server_only`, and `@default(...)` columns are excluded by
/// construction.
pub(super) async fn upsert_returning_record<'e, E, M, PK>(
    executor: E,
    descriptor: &'static ModelDescriptor<M, PK>,
    insert_values: &[SqlColumnValue],
    conflict_target: ConflictTarget,
) -> Result<M, CratestackError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow>,
{
    let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("INSERT INTO ");
    query.push(descriptor.table_name).push(" (");
    for (index, value) in insert_values.iter().enumerate() {
        if index > 0 {
            query.push(", ");
        }
        query.push(value.column);
    }
    query.push(") VALUES (");
    for (index, value) in insert_values.iter().enumerate() {
        if index > 0 {
            query.push(", ");
        }
        push_bind_value(&mut query, &value.value);
    }

    query.push(") ON CONFLICT (");
    match conflict_target.as_columns() {
        None => {
            query.push(descriptor.primary_key);
        }
        Some(cols) => {
            for (idx, column) in cols.iter().enumerate() {
                if idx > 0 {
                    query.push(", ");
                }
                query.push(*column);
            }
        }
    }
    query.push(")");
    // Unpredicated targets emit byte-identical SQL to before cratestack#741 —
    // this branch is a no-op when `predicate()` is `None`.
    if let Some(predicate) = conflict_target.predicate() {
        query.push(" WHERE ").push(predicate);
    }
    query.push(" DO UPDATE SET ");

    // If there are no eligible columns to overwrite, fall back to a
    // no-op assignment: touching the PK to itself. This keeps the
    // RETURNING clause working (PG only RETURNs from rows the
    // statement touched).
    //
    // This is deliberately NOT the same mechanism as the per-call-site
    // `.do_nothing()` builder (`upsert_do_nothing_sql::
    // upsert_returning_record_do_nothing`, cratestack#487). They solve
    // different problems and stay separate on purpose:
    //   * This fallback is keyed off `descriptor.upsert_update_columns`
    //     being empty — a property of the *model*, not the call site —
    //     and it is still a `DO UPDATE`. Postgres treats the
    //     self-assignment as a real write: `xmax` bumps, `updated_at`-
    //     style `BEFORE UPDATE` triggers fire, and it generates WAL/
    //     logical-replication traffic, even though no column value
    //     actually changes.
    //   * `.do_nothing()` is an explicit opt-in *per call*, independent
    //     of the model's `upsert_update_columns`, and compiles to a
    //     genuine `ON CONFLICT ... DO NOTHING` — the row is untouched
    //     at the storage layer, no triggers fire, no WAL is generated
    //     for the conflicting row. That distinction is the entire point
    //     of #487: a model with updatable columns had no way to ask for
    //     real non-mutation on a specific call.
    // Merging them would force every empty-`upsert_update_columns`
    // model onto DO-NOTHING semantics (a silent behavior change for
    // existing consumers) or force `.do_nothing()` callers to pay for
    // trigger fan-out they explicitly asked to avoid. Keeping both is a
    // few lines of duplication in exchange for neither call site
    // silently inheriting the other's semantics.
    if descriptor.upsert_update_columns.is_empty() {
        query.push(descriptor.primary_key);
        query.push(" = EXCLUDED.").push(descriptor.primary_key);
    } else {
        for (index, column) in descriptor.upsert_update_columns.iter().enumerate() {
            if index > 0 {
                query.push(", ");
            }
            query.push(*column).push(" = EXCLUDED.").push(*column);
        }
    }
    if let Some(version_col) = descriptor.version_column {
        query
            .push(", ")
            .push(version_col)
            .push(" = ")
            .push(descriptor.table_name)
            .push(".")
            .push(version_col)
            .push(" + 1");
    }

    query
        .push(" RETURNING ")
        .push(descriptor.select_projection());

    query
        .build_query_as::<M>()
        .fetch_one(executor)
        .await
        .map_err(classify_unique_violation)
}
