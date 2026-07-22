//! Structural diff between two parsed `.cstack` schemas, classified by
//! wire-contract impact rather than raw text differences.
//!
//! Breaking-change taxonomy (see the `cratestack-docs` "Schema diff"
//! page for the full write-up):
//!
//! * Removing a model, field, or procedure is breaking — a consumer
//!   referencing it stops compiling/working.
//! * Adding `@@paged` to a model is breaking: it changes that model's
//!   `.list()` response envelope from `T[]` to `Page<T>` (and vice
//!   versa for removal).
//! * Adding a model, an optional field, or a procedure is additive.
//! * Adding a *required* field/argument with no default is breaking:
//!   existing client-constructed payloads that omit it are rejected.
//! * Retyping a field/arg/return type, or narrowing arity
//!   (optional → required, or introducing/removing `[]`), is
//!   breaking; widening (required → optional) is additive.
//! * Other model-level attributes (`@@soft_delete`, `@@audit`,
//!   `@@retain`, `@@emit`) are tracked as internal-only for now — they
//!   affect server behavior, not the shape of the wire contract this
//!   tool models. This is a known scope gap, not an oversight.

mod arity;
mod fields;
mod models;
mod procedures;
mod render;

#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests_attributes;
#[cfg(test)]
mod tests_models;
#[cfg(test)]
mod tests_procedures;

use cratestack_core::Schema;
use serde::Serialize;

pub(crate) use render::{render_human, render_json};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Severity {
    /// Changes the wire contract in a way existing consumers can't
    /// tolerate.
    Breaking,
    /// Extends the wire contract without disturbing existing
    /// consumers.
    Additive,
    /// Recognised change with no tracked wire-shape effect.
    Internal,
}

impl Severity {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Severity::Breaking => "BREAKING",
            Severity::Additive => "ADDITIVE",
            Severity::Internal => "INTERNAL",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct Change {
    pub(crate) severity: Severity,
    pub(crate) category: &'static str,
    pub(crate) subject: String,
    pub(crate) message: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub(crate) struct SchemaDiff {
    pub(crate) changes: Vec<Change>,
}

impl SchemaDiff {
    pub(crate) fn has_breaking(&self) -> bool {
        self.changes
            .iter()
            .any(|change| change.severity == Severity::Breaking)
    }

    /// Returns `(breaking, additive, internal)` counts.
    pub(crate) fn counts(&self) -> (usize, usize, usize) {
        let mut counts = (0, 0, 0);
        for change in &self.changes {
            match change.severity {
                Severity::Breaking => counts.0 += 1,
                Severity::Additive => counts.1 += 1,
                Severity::Internal => counts.2 += 1,
            }
        }
        counts
    }
}

/// Structurally diffs `prev` against `next`, matching models/fields/
/// procedures by name only (no rename inference — a rename looks like
/// a removal plus an addition, mirroring `cratestack-migrate`'s DB
/// diffing philosophy).
pub(crate) fn diff_schemas(prev: &Schema, next: &Schema) -> SchemaDiff {
    let mut changes = Vec::new();
    models::diff_models(prev, next, &mut changes);
    procedures::diff_procedures(prev, next, &mut changes);
    SchemaDiff { changes }
}
