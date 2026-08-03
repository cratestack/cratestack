//! CHECK-constraint IR. A CHECK constraint is promoted either from a
//! `.cstack` validator marked `@db_enforce`, or from an `enum`-typed
//! field (see [`CheckKind::Enum`]); the IR captures the *kind* of
//! constraint so each emitter renders the predicate in its own
//! dialect, rather than a raw SQL fragment.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddCheck {
    pub table: String,
    pub column: String,
    pub name: String,
    pub kind: CheckKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DropCheck {
    pub table: String,
    pub column: String,
    pub name: String,
    /// The predicate being dropped. Carried so emitters can decide
    /// whether the constraint had any footprint in their dialect at
    /// all (SQLite never materialises [`CheckKind::Enum`]), and so
    /// down-emission can reconstruct the `ADD CONSTRAINT` it reverses.
    pub kind: CheckKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CheckKind {
    /// `@range(min, max)` — numeric bounds. Either bound may be absent.
    Range { min: Option<i64>, max: Option<i64> },
    /// `@length(min, max)` — string/bytes length bounds.
    Length { min: Option<i64>, max: Option<i64> },
    /// `@iso4217` — three ASCII uppercase letters.
    Iso4217,
    /// Membership constraint standing in for a native enum type.
    ///
    /// Enum-typed columns are stored as `TEXT` on every backend
    /// (issue #228: the generated row decoders read enums via
    /// `try_get::<String>` and `.parse()`, so a native Postgres enum
    /// column fails to decode at runtime). This constraint recovers
    /// the validation the native type would have provided.
    ///
    /// `variants` is kept in declaration order and is never empty —
    /// the projection skips the check entirely for a variant-less
    /// enum rather than emitting a degenerate `IN ()`.
    Enum {
        variants: Vec<String>,
        /// Whether the column is a list (`Status[]` → `TEXT[]`), which
        /// needs array containment rather than scalar membership.
        list: bool,
    },
    /// An opaque CHECK predicate, captured verbatim as SQL text rather
    /// than reverse-mapped to one of the kinds above.
    ///
    /// Only ever produced by live-database introspection (Phase B,
    /// issue #204) — `.cstack` schema projection never emits this
    /// variant, since every validator it knows how to compile
    /// (`@range`, `@length`, `@iso4217`) already has a precise
    /// `CheckKind`. A CHECK constraint an introspector finds in
    /// `pg_constraint` that doesn't match the enum-membership shape
    /// (see `crate::introspect::postgres`) could be *any* SQL
    /// predicate — including one of the validator-derived kinds above,
    /// since design doc §2.2 notes the compiled SQL for e.g.
    /// `@range(0, 150)` is indistinguishable from hand-written
    /// `CHECK (age >= 0 AND age <= 150)` once it reaches the catalog.
    /// Rather than guess which validator (if any) produced it,
    /// introspection always reports it as opaque text — surfaced by
    /// the diff engine as drift against whatever `.cstack` declares
    /// for the same column, which is the correct, conservative
    /// behavior per the "unmapped → reported drift, never guessed"
    /// rule.
    Raw(String),
}
