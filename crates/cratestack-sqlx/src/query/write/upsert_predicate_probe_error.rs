//! Classifying [`super::upsert_predicate_probe`]'s own query errors
//! (cratestack#741 finding 2 follow-up). Split out of
//! `upsert_predicate_probe.rs` purely to stay under this codebase's
//! ~200-LoC-per-file convention, not a behavioral boundary.
//!
//! # Actionable error for the `42703` case
//!
//! When the predicate references a column absent from `insert_values`
//! (typically a `@default(...)` column — see
//! `upsert_predicate_probe_savepoint`'s doc comment), the derived-table
//! `SELECT` [`super::upsert_predicate_probe::incoming_row_satisfies_predicate_inner`]
//! builds fails with Postgres `42703 column "..." does not exist`.
//! [`classify_probe_error`] special-cases exactly that SQLSTATE, from
//! exactly this one query, into a [`CratestackError::Validation`]
//! naming the predicate and the likely cause/workaround — not a
//! generic [`cratestack_error_from_sqlx`] mapping (which would leave
//! the caller staring at a raw `DatabaseTyped` 500 with no indication
//! of what to change). This is deliberately narrow: only this probe
//! query, only `42703`; every other SQLSTATE, and every other call
//! site in this crate, keeps using `cratestack_error_from_sqlx`
//! unchanged — broadening that function itself would change unrelated
//! error surfaces that have nothing to do with this probe.
//!
//! # Provably narrow, not incidentally safe
//!
//! [`ProbeOutcome`] exists so the DO NOTHING path's fallback-on-failure
//! behavior (`upsert_predicate_probe_savepoint::try_incoming_row_satisfies_predicate`)
//! is narrow **by construction**, not by the accident that no other
//! error currently happens to reach it. [`classify_probe_error`] is the
//! single point that decides which failures mean "this predicate
//! cannot be evaluated against a synthetic one-row derived table"
//! (`ProbeOutcome::UndefinedColumn`) versus everything else
//! (`ProbeOutcome::Other`) — connection loss, statement timeout
//! (`57014`), a permission error, a poisoned transaction, a genuinely
//! malformed predicate under any other SQLSTATE, etc. Only
//! `UndefinedColumn` is ever treated as "unknown, fall back"; `Other`
//! always propagates. There is currently no SQLSTATE "sibling" to
//! `42703` worth adding to the narrow set: the derived table wraps
//! exactly `insert_values`, so the only way a predicate can fail
//! specifically *because of* that synthetic shape is a reference to a
//! column that isn't in it — an undefined function, a type mismatch, a
//! syntax error, etc. are all genuine predicate-authoring bugs, not
//! artifacts of the derived table, and must surface as errors rather
//! than being silently absorbed.

use cratestack_core::CratestackError;

use crate::{cratestack_error_from_sqlx, sqlx};

/// Postgres SQLSTATE for "undefined column".
const UNDEFINED_COLUMN_SQLSTATE: &str = "42703";

/// The two classes of failure
/// [`super::upsert_predicate_probe::incoming_row_satisfies_predicate_inner`]'s
/// query can produce, kept as distinct variants (rather than a single
/// `CratestackError`) so callers that need to discriminate — currently
/// only `try_incoming_row_satisfies_predicate`'s savepoint fallback —
/// can `match` on this enum instead of inspecting the mapped error's
/// shape and hoping no future change to [`classify_probe_error`] makes
/// that inspection wrong.
pub(super) enum ProbeOutcome {
    /// Postgres `42703` (undefined column) — the ONLY class
    /// `try_incoming_row_satisfies_predicate` treats as "unknown, fall
    /// back to the authoritative statement".
    UndefinedColumn(CratestackError),
    /// Every other failure. Always propagates as-is.
    Other(CratestackError),
}

impl ProbeOutcome {
    /// Unwrap to the underlying `CratestackError`, discarding which
    /// class it was. Used by [`super::upsert_predicate_probe::incoming_row_satisfies_predicate`],
    /// the DO UPDATE path's caller, which has no savepoint fallback and
    /// always propagates either class identically.
    pub(super) fn into_error(self) -> CratestackError {
        match self {
            Self::UndefinedColumn(error) | Self::Other(error) => error,
        }
    }
}

/// Maps an error from the incoming-row probe's own query into a
/// [`ProbeOutcome`]. See the module doc comment for the full reasoning
/// on why `42703` is special-cased and why nothing else is.
pub(super) fn classify_probe_error(predicate: &'static str, error: sqlx::Error) -> ProbeOutcome {
    if let sqlx::Error::Database(ref db_err) = error
        && db_err.code().as_deref() == Some(UNDEFINED_COLUMN_SQLSTATE)
    {
        return ProbeOutcome::UndefinedColumn(CratestackError::Validation(format!(
            "upsert conflict predicate `{predicate}` references a column that is not present \
             in the insert values, so it could not be evaluated before the row exists (Postgres: \
             {detail}). This usually means that column carries `@default(...)` in the schema — \
             any `@default(...)`, not just an `auth()`-derived one, excludes the field from the \
             generated create input, so the database's own column DEFAULT fills it and this \
             predicate can never see its value client-side. Fix by either supplying the column \
             explicitly in the input, or writing a predicate that only references columns the \
             input always carries.",
            detail = db_err.message(),
        )));
    }
    ProbeOutcome::Other(cratestack_error_from_sqlx(error))
}
