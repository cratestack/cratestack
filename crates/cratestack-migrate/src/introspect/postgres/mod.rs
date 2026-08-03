//! Postgres live-schema introspection (issue #204).
//!
//! [`introspect`] queries a live [`PgPool`]'s `information_schema` /
//! `pg_catalog` state and produces an [`IntrospectionReport`] whose
//! [`IntrospectionReport::projections`] field is exactly the
//! [`crate::Projections`] shape [`crate::project`] would produce from
//! an equivalent, hand-authored `.cstack` schema — see each submodule
//! for the specific catalog queries and the reasoning behind them:
//!
//! * [`tables`] — table list (`pg_class`, excluding `cratestack_migrations`).
//! * [`columns`] — column shape (`pg_attribute`/`pg_type`/`pg_attrdef`),
//!   mapped through [`types::map_scalar`]'s common-scalar whitelist.
//! * [`indexes`] — non-PK indexes (`pg_index`).
//! * [`constraints`] — primary key + CHECK constraints (`pg_constraint`).
//! * [`enums`] — native Postgres enum types (`pg_enum`/`pg_type`),
//!   folded into a CHECK the same way schema-side projection does.
//! * [`views`] — view definitions (`pg_views`-equivalent catalog
//!   queries) and their source tables (`pg_depend`).
//!
//! # Known gaps (surface these clearly in Phase C's drift report)
//!
//! * **Foreign keys are not introspected.** Neither the issue nor the
//!   design doc's §5.2 query list mentions `pg_constraint`'s
//!   `contype = 'f'` rows, so [`crate::TableProjection::foreign_keys`]
//!   is always empty here. A table with `.cstack`-declared relations
//!   will show every foreign key as "missing" drift until a follow-up
//!   phase adds this.
//! * **Multi-column and zero-column CHECK constraints are skipped.**
//!   [`crate::ir::AddCheck`] ties to exactly one column — there's no
//!   IR shape for `CHECK (a < b)` — so [`constraints::introspect_checks`]
//!   only considers `contype = 'c'` rows with `array_length(conkey, 1)
//!   = 1`; anything else is silently absent from the result rather
//!   than mis-attributed to one of its columns.
//! * **Expression and partial indexes are skipped**, not guessed at —
//!   see [`indexes`]'s doc comment.
//! * **A view's `@id` field can't be recovered** — see [`views`]'s doc
//!   comment; introspected views always report an empty primary key.
//! * **Enum-typed `.cstack` columns don't round-trip their column
//!   type**, only their CHECK — see [`enums`]'s doc comment. An
//!   introspected enum-backed column always reports
//!   `ColumnType::Scalar("String")`, never `ColumnType::Enum(name)`,
//!   because the `.cstack`-side enum name has no catalog trace to
//!   recover it from.
//! * **Default-value text is best-effort normalised**, not a full SQL
//!   parser — see `columns::parse_default`'s doc comment.

mod check_pattern;
mod columns;
mod constraints;
mod enums;
mod error;
mod indexes;
mod report;
mod tables;
mod types;
mod views;

pub use error::IntrospectError;
pub use report::{IntrospectionReport, UnmappedColumn};

use sqlx_postgres::PgPool;

use crate::TableProjection;

use columns::ColumnOutcome;
use report::IntrospectionReportBuilder;

/// Introspect every table and view `pool`'s current schema owns
/// (`current_schema()` — respecting `search_path` the same way
/// `cratestack-studio`'s existing Postgres data source does) and
/// return the [`Projections`](crate::Projections) shape, plus any
/// columns whose type couldn't be confidently mapped.
///
/// Read-only: issues no DDL and starts no transaction. Safe to run
/// against a database under concurrent write load, with the same
/// caveat any catalog snapshot has — the result reflects the schema at
/// query time, not a single atomic instant across every table.
pub async fn introspect(pool: &PgPool) -> Result<IntrospectionReport, IntrospectError> {
    let mut report = IntrospectionReportBuilder::default();

    for table in tables::list_tables(pool).await? {
        let mut columns = Vec::new();
        for outcome in columns::introspect_columns(pool, &table).await? {
            match outcome {
                ColumnOutcome::Mapped(column) => columns.push(column),
                ColumnOutcome::Unmapped {
                    column,
                    postgres_type,
                } => report.unmapped_column(&table, column, postgres_type),
            }
        }

        let pk_columns = constraints::introspect_primary_key(pool, &table).await?;
        for column in &mut columns {
            column.primary_key = pk_columns.contains(&column.name);
        }

        let mut checks = constraints::introspect_checks(pool, &table).await?;
        checks.extend(enums::introspect_enum_checks(pool, &table).await?);
        let indexes = indexes::introspect_indexes(pool, &table).await?;

        report.table(TableProjection {
            name: table.clone(),
            rename_from: None,
            columns,
            column_renames: Vec::new(),
            indexes,
            checks,
            foreign_keys: Vec::new(),
        });
    }

    for view in views::introspect_views(pool).await? {
        report.view(view);
    }

    Ok(report.finish())
}
