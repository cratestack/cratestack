//! Output shape of [`super::introspect`].

use crate::{Projections, TableProjection, ViewProjection};

/// Result of introspecting a live Postgres database.
///
/// [`Self::projections`] is the diffable IR — feed it directly to
/// [`crate::diff_projections`] alongside [`crate::project`]'s output
/// for the current `.cstack` schema to get a drift report (Phase C,
/// issue #205). [`Self::unmapped_columns`] is a side channel: columns
/// whose Postgres type isn't in the common mapped set are left out of
/// `projections` entirely rather than guessed at (design doc §5.2), so
/// they're surfaced here instead — a real column the schema comparison
/// can't see, which Phase C's CLI needs to print loudly rather than
/// silently drop.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IntrospectionReport {
    pub projections: Projections,
    pub unmapped_columns: Vec<UnmappedColumn>,
}

/// A column whose Postgres type could not be confidently mapped to a
/// `.cstack` scalar. Never guessed at — see `super::types::map_scalar`'s
/// doc comment for the exact whitelist and rationale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnmappedColumn {
    pub table: String,
    pub column: String,
    /// The Postgres type name as reported by `pg_type.typname` (e.g.
    /// `"numeric"`, `"jsonb"`, `"_text"` for `text[]`, `"int4"`).
    pub postgres_type: String,
}

/// Accumulates one [`IntrospectionReport`] across `super::introspect`'s
/// per-table/per-view loop. Kept private to this module so the
/// orchestrator can't accidentally hand back a half-built report —
/// [`Self::finish`] is the only way to get an [`IntrospectionReport`]
/// out.
#[derive(Default)]
pub(super) struct IntrospectionReportBuilder {
    report: IntrospectionReport,
}

impl IntrospectionReportBuilder {
    pub(super) fn table(&mut self, table: TableProjection) {
        self.report
            .projections
            .tables
            .insert(table.name.clone(), table);
    }

    pub(super) fn view(&mut self, view: ViewProjection) {
        self.report
            .projections
            .views
            .insert(view.name.clone(), view);
    }

    pub(super) fn unmapped_column(&mut self, table: &str, column: String, postgres_type: String) {
        self.report.unmapped_columns.push(UnmappedColumn {
            table: table.to_owned(),
            column,
            postgres_type,
        });
    }

    pub(super) fn finish(self) -> IntrospectionReport {
        self.report
    }
}
