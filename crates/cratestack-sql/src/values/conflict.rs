use cratestack_core::CratestackError;

/// Conflict target for an upsert. Defaults to the model's primary key
/// (matching the previous PK-only behavior). [`Self::Columns`] /
/// [`Self::columns`] let callers upsert on an arbitrary unique tuple —
/// most commonly a natural key that's distinct from the PK (e.g.
/// `(owner_id, provider)` on a per-owner-and-provider settings row, or
/// `(pairing_id, slot)` on a per-slot envelope).
///
/// The named columns MUST correspond to a `UNIQUE` constraint or
/// `UNIQUE` index on the target table — the database engine enforces
/// this and will surface a clear error if not. The upsert builder
/// additionally requires the input to carry a value for every column
/// in the target tuple, so the conflict probe (`SELECT … FOR UPDATE`)
/// has something to filter on. A column that IS present but whose
/// value is one of the `SqlValue::Null*` variants satisfies this
/// requirement — the probe then filters on `column = NULL`, which
/// never matches any row (three-valued SQL logic), so an upsert keyed
/// on a NULL natural-key column always takes the insert branch. That
/// is deliberate, not merely convenient: it is the same rule a `WHERE
/// col IS NOT NULL` partial index encodes, so a NULL key naturally
/// falls outside such an index's uniqueness domain both in Postgres's
/// own `ON CONFLICT` inference and in this crate's conflict probe.
///
/// Composite-constraint-by-name (`ON CONFLICT ON CONSTRAINT
/// my_unique_idx_v2`) is not yet exposed; pass the matching column
/// tuple via [`Self::Columns`] instead.
///
/// # Partial unique indexes (cratestack#741)
///
/// [`Self::where_index`] attaches an index predicate so an upsert can
/// target a **partial** unique index (`CREATE UNIQUE INDEX ... WHERE
/// <predicate>`) — Postgres will not infer a partial index from an
/// unpredicated `ON CONFLICT (<cols>)`, so without this the statement
/// fails at runtime with "there is no unique or exclusion constraint
/// matching the ON CONFLICT specification". The predicate is kept a
/// `&'static str`, exactly like the column names: a compile-time
/// constant from the schema/call site, passed through to the database
/// verbatim, with no runtime-value path into the rendered SQL (the
/// same precedent `@@index`'s `using`/`opclass` already set).
///
/// The predicate is not just appended to the emitted `ON CONFLICT (…)
/// WHERE …` clause — every conflict probe this crate's runtimes issue
/// to decide `Inserted` vs. `Existing`/`DO UPDATE` also applies it.
/// Skipping that half would let the probe match a row the partial
/// index does not cover, handing the caller a wrong verdict even
/// though the emitted SQL looks correct.
///
/// Declaring a partial index in the schema DDL (`@@unique([...],
/// where: "...")`) is a separate concern (cratestack#742): this type
/// only lets an upsert *target* a partial index that already exists.
///
/// # Why an enum with four variants, not two plus a predicate field
///
/// An earlier draft of this ticket's fix collapsed this type into a
/// `{ kind, predicate }` struct, which deleted the public `PrimaryKey`
/// and `Columns(&'static [&'static str])` variants direct construction
/// / pattern-matching relied on. A repo-wide grep showed every in-repo
/// call site only ever *constructs* a `ConflictTarget` (never pattern-
/// matches one), so that break bought nothing — the maintainer ruled
/// this be reworked additively instead (cratestack#741 finding 3):
/// [`Self::PrimaryKey`] and [`Self::Columns`] are restored exactly as
/// they were pre-#741, and the predicate rides along on two new,
/// purely additive variants ([`Self::ColumnsWithPredicate`],
/// [`Self::PrimaryKeyWithPredicate`]) reached through
/// [`Self::where_index`] rather than constructed directly. The invalid
/// `PrimaryKey` + predicate combination deliberately stays
/// *representable* (via [`Self::PrimaryKeyWithPredicate`]) rather than
/// being ruled out at the type level, so [`Self::validate`] can reject
/// it at runtime with a clear [`CratestackError::Validation`] instead
/// of the type system silently preventing the chain
/// `PrimaryKey.where_index(..)` from being written at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConflictTarget {
    /// The model's `@id` primary key, unpredicated. Default.
    #[default]
    PrimaryKey,
    /// A caller-supplied tuple of columns forming a unique key on the
    /// target table, unpredicated. Chain [`Self::where_index`] to
    /// target a partial unique index instead of a plain one.
    Columns(&'static [&'static str]),
    /// Additive (cratestack#741): [`Self::Columns`] with an attached
    /// partial-unique-index predicate. Reached via
    /// `ConflictTarget::columns(&[...]).where_index(<predicate>)`; not
    /// normally constructed directly.
    ColumnsWithPredicate(&'static [&'static str], &'static str),
    /// Additive (cratestack#741): [`Self::PrimaryKey`] with an
    /// attached predicate. This combination can never correspond to a
    /// real index — the primary key index is never partial — and
    /// [`Self::validate`] always rejects it. It stays representable
    /// (rather than prevented at the type level) so that rejection is
    /// a normal runtime [`CratestackError`], not a call that can't
    /// even be written. Reached via
    /// `ConflictTarget::PRIMARY_KEY.where_index(<predicate>)`; not
    /// normally constructed directly.
    PrimaryKeyWithPredicate(&'static str),
}

impl ConflictTarget {
    /// `ConflictTarget::PrimaryKey` as an associated const, matching
    /// the naming convention [`Self::columns`] sets for `Columns`.
    /// Kept alongside the `PrimaryKey` variant itself (both spellings
    /// are used across this codebase's call sites and tests).
    pub const PRIMARY_KEY: Self = Self::PrimaryKey;

    /// Sugar for `ConflictTarget::Columns(&[...])`.
    pub const fn columns(cols: &'static [&'static str]) -> Self {
        Self::Columns(cols)
    }

    /// Attach a partial-unique-index predicate, e.g.
    /// `ConflictTarget::columns(&["k"]).where_index("status = 'active'")`
    /// for an index declared as `UNIQUE (k) WHERE status = 'active'`.
    ///
    /// Only valid when chained onto [`Self::Columns`]/[`Self::columns`]
    /// — the primary key index is never partial, so chaining this onto
    /// [`Self::PrimaryKey`]/[`Self::PRIMARY_KEY`] is rejected by
    /// [`Self::validate`] rather than silently dropped. This method
    /// itself stays infallible (`const fn`, so it can be used in a
    /// `const` builder chain) — the rejection happens where the target
    /// is actually consumed, before any SQL is built.
    pub const fn where_index(self, predicate: &'static str) -> Self {
        match self {
            Self::PrimaryKey | Self::PrimaryKeyWithPredicate(_) => {
                Self::PrimaryKeyWithPredicate(predicate)
            }
            Self::Columns(cols) | Self::ColumnsWithPredicate(cols, _) => {
                Self::ColumnsWithPredicate(cols, predicate)
            }
        }
    }

    /// The attached partial-index predicate, if any.
    pub const fn predicate(&self) -> Option<&'static str> {
        match self {
            Self::ColumnsWithPredicate(_, predicate) | Self::PrimaryKeyWithPredicate(predicate) => {
                Some(*predicate)
            }
            Self::PrimaryKey | Self::Columns(_) => None,
        }
    }

    /// `true` when this target is the model's primary key.
    pub const fn is_primary_key(&self) -> bool {
        matches!(self, Self::PrimaryKey | Self::PrimaryKeyWithPredicate(_))
    }

    /// The column tuple, if this target is [`Self::Columns`] /
    /// [`Self::columns`] (predicated or not); `None` for
    /// [`Self::PrimaryKey`]/[`Self::PRIMARY_KEY`].
    pub const fn as_columns(&self) -> Option<&'static [&'static str]> {
        match self {
            Self::Columns(cols) | Self::ColumnsWithPredicate(cols, _) => Some(*cols),
            Self::PrimaryKey | Self::PrimaryKeyWithPredicate(_) => None,
        }
    }

    /// Reject a predicate paired with the primary key target — the PK
    /// index is never partial, so that combination can never
    /// correspond to a real index. Every runtime entry point that
    /// consumes a `ConflictTarget` calls this before doing any SQL
    /// work, so the rejection is a clear
    /// [`CratestackError::Validation`], not a silently dropped
    /// predicate or a confusing database-side error.
    pub fn validate(&self) -> Result<(), CratestackError> {
        if self.is_primary_key() && self.predicate().is_some() {
            return Err(CratestackError::Validation(
                "ConflictTarget predicate requires ConflictTarget::columns(...); the primary \
                 key index is never partial, so a predicate on ConflictTarget::PRIMARY_KEY \
                 cannot correspond to any real index"
                    .to_owned(),
            ));
        }
        Ok(())
    }
}
