//! Return type for `.upsert(..).do_nothing()` (cratestack#487).
//!
//! A genuine `ON CONFLICT ... DO NOTHING` returns nothing at all for
//! the conflicting row — Postgres only RETURNs rows a statement
//! actually touched, and DO NOTHING touches none. Callers therefore
//! need "inserted" and "already existed" to be distinguishable in the
//! type, not collapsed into a single `M` the way the DO UPDATE path's
//! `.upsert(..).run(..)` returns it.

/// Outcome of a `.upsert(..).do_nothing().run(..)` call.
///
/// # Race semantics
///
/// The runtime always resolves the conflict under a `SELECT ... FOR
/// UPDATE` row lock held for the lifetime of the surrounding
/// transaction (see `upsert_do_nothing_exec::run_upsert_do_nothing_in_tx`
/// for the exact sequencing):
///
/// * If the probe finds an existing row, that row is locked before this
///   call returns — no concurrent transaction can delete or modify it
///   until the caller commits — so [`Existing`](Self::Existing) is a
///   guarantee about the row's state *at the moment this call
///   returns*, not merely "at some point during the call".
/// * If the probe finds nothing, the actual `INSERT ... ON CONFLICT
///   DO NOTHING` is still the statement that runs (not a plain
///   `INSERT`), because the probe's "no row" answer does not itself
///   lock anything — a concurrent transaction can commit a conflicting
///   row in the gap between the probe and the INSERT. When that race
///   is lost, the runtime performs one more locked read to hand back
///   the row the other transaction actually committed, so callers never
///   see a phantom "existing" row invented from stale data — see
///   [`Existing`](Self::Existing) below for what happens if *that* row
///   is deleted before the fallback read completes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpsertOutcome<M> {
    /// This call performed the insert. No row previously existed at
    /// the conflict target.
    Inserted(M),
    /// A row already existed at the conflict target and was left
    /// completely untouched by this call — no columns written, no
    /// `Updated` event emitted, no audit entry recorded, because
    /// nothing about the row changed.
    ///
    /// If this outcome was reached via the race-fallback path (the
    /// insert branch lost a concurrent-insert race and had to read the
    /// winning row back), and *that* row was deleted before the
    /// fallback read could complete, the call surfaces
    /// `CoolError::Conflict` instead of ever constructing an
    /// `Existing` from data that might not be current — see
    /// `upsert_do_nothing_exec` for that narrower race.
    Existing(M),
}

impl<M> UpsertOutcome<M> {
    /// `true` for [`Self::Inserted`].
    pub fn was_inserted(&self) -> bool {
        matches!(self, Self::Inserted(_))
    }

    /// Discard the inserted-vs-existing distinction and take the row.
    /// Prefer matching on the enum when the distinction matters (that's
    /// the entire reason this type exists) — this is for callers that
    /// only ever need the record, e.g. read-modify-report call sites
    /// that already branched on [`Self::was_inserted`].
    pub fn into_record(self) -> M {
        match self {
            Self::Inserted(record) | Self::Existing(record) => record,
        }
    }

    /// Borrow the record regardless of which variant this is.
    pub fn record(&self) -> &M {
        match self {
            Self::Inserted(record) | Self::Existing(record) => record,
        }
    }
}
