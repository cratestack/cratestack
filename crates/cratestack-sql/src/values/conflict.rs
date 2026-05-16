/// Conflict target for an upsert. Defaults to the model's primary key
/// (matching the previous PK-only behavior). `Columns` lets callers
/// upsert on an arbitrary unique tuple — most commonly a natural key
/// that's distinct from the PK (e.g. `(owner_id, provider)` on a
/// per-owner-and-provider settings row, or `(pairing_id, slot)` on a
/// per-slot envelope).
///
/// The named columns MUST correspond to a `UNIQUE` constraint or
/// `UNIQUE` index on the target table — the database engine enforces
/// this and will surface a clear error if not. The upsert builder
/// additionally requires the input to carry a value for every column
/// in the target tuple, so the conflict probe (`SELECT … FOR UPDATE`)
/// has something to filter on.
///
/// Composite-constraint-by-name (`ON CONFLICT ON CONSTRAINT
/// my_unique_idx_v2`) is not yet exposed; pass the matching column
/// tuple via [`Self::Columns`] instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictTarget {
    /// The model's `@id` primary key. Default.
    PrimaryKey,
    /// A caller-supplied tuple of columns forming a unique key on the
    /// target table.
    Columns(&'static [&'static str]),
}

impl ConflictTarget {
    /// Sugar for `ConflictTarget::Columns(&[...])`.
    pub const fn columns(cols: &'static [&'static str]) -> Self {
        Self::Columns(cols)
    }
}

impl Default for ConflictTarget {
    fn default() -> Self {
        Self::PrimaryKey
    }
}
