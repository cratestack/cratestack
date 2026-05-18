//! View / materialized-view diff (ADR-0003).
//!
//! Projects each `view` block in `prev` and `next` into a
//! [`ViewProjection`] and emits `CreateView` / `ReplaceView` /
//! `DropView` (or the materialized variants) ops. The view's SQL
//! body is dialect-specific; the projection picks `server_sql()`
//! or `embedded_sql()` based on `Schema.datasource.provider`. A
//! view that only declares the off-dialect body is silently
//! dropped from the projection — same way the macro composers
//! treat backend-specific views.
//!
//! Ordering invariants (enforced by the caller in
//! [`super::diff`]):
//!
//! - View drops are appended **before** table drops (the source
//!   tables they depend on get dropped after).
//! - View creates are appended **after** table creates (sources
//!   exist before the view references them).

use std::collections::BTreeMap;

use cratestack_core::{Schema, View};

use crate::ir::{CreateMaterializedView, CreateView, DropMaterializedView, DropView, Op};

/// Internal projection of a view at one side of a diff. Carries the
/// dialect-specific SQL body chosen at projection time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ViewProjection {
    pub(super) name: String,
    pub(super) sql: String,
    pub(super) is_materialized: bool,
    pub(super) primary_key: String,
    pub(super) source_tables: Vec<String>,
}

pub(super) fn project_views(schema: &Schema) -> BTreeMap<String, ViewProjection> {
    let dialect = dialect_for(schema);
    schema
        .views
        .iter()
        .filter_map(|view| project_view(view, dialect).map(|p| (p.name.clone(), p)))
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dialect {
    Postgres,
    Sqlite,
}

fn dialect_for(schema: &Schema) -> Dialect {
    let provider = schema
        .datasource
        .as_ref()
        .and_then(|ds| {
            ds.entries
                .iter()
                .find(|entry| entry.key == "provider")
                .map(|entry| entry.value.trim_matches('"').to_owned())
        })
        .unwrap_or_default();
    match provider.as_str() {
        "sqlite" => Dialect::Sqlite,
        // Default to Postgres for unknown / unspecified providers —
        // matches the historical behavior of the macro composers.
        _ => Dialect::Postgres,
    }
}

fn project_view(view: &View, dialect: Dialect) -> Option<ViewProjection> {
    // Materialized views are server-only (ADR-0003 §"Materialized
    // views"). The macro's embedded composer hard-errors at expansion
    // time; the migration generator has to apply the same gate
    // independently because it doesn't go through the macro. Without
    // this filter, a schema with `@@materialized` + `@@sql(...)`
    // pointed at `provider = "sqlite"` would reach the SQLite emitter's
    // `unreachable!`.
    if matches!(dialect, Dialect::Sqlite) && view.is_materialized() {
        return None;
    }
    let sql = match dialect {
        Dialect::Postgres => view.server_sql()?.to_owned(),
        Dialect::Sqlite => view.embedded_sql()?.to_owned(),
    };
    let source_tables = view
        .sources
        .iter()
        .map(|src| crate::naming::table_name(&src.name))
        .collect();
    let primary_key = view
        .fields
        .iter()
        .find(|field| field.attributes.iter().any(|attr| attr.raw == "@id"))
        .map(|field| crate::naming::column_name(&field.name))
        .unwrap_or_default();
    Some(ViewProjection {
        name: crate::naming::table_name(&view.name),
        sql,
        is_materialized: view.is_materialized(),
        primary_key,
        source_tables,
    })
}

/// Diff bucket — drops are flushed before column/table drops, creates
/// after column/table creates (see `super::diff`).
///
/// Body changes are modelled as a `Drop + Create` pair rather than a
/// `ReplaceView` op. This sacrifices the atomicity of Postgres's
/// `CREATE OR REPLACE VIEW` for ordering correctness when the same
/// migration drops a column the old body referenced or adds a column
/// the new body references — the `Drop` lands before column drops
/// (old body can't block the column DROP) and the `Create` lands
/// after column adds (new body's column refs are valid). Within a
/// single Postgres migration transaction, other connections never
/// observe the transient `view missing` state.
#[derive(Debug, Default)]
pub(super) struct ViewDiff {
    pub(super) drops: Vec<Op>,
    pub(super) creates: Vec<Op>,
}

pub(super) fn diff_views(prev: &Schema, next: &Schema) -> ViewDiff {
    let prev_views = project_views(prev);
    let next_views = project_views(next);
    let mut diff = ViewDiff::default();

    // Drops: anything in prev that's not in next.
    for (name, prev_proj) in &prev_views {
        if !next_views.contains_key(name) {
            diff.drops.push(if prev_proj.is_materialized {
                Op::DropMaterializedView(DropMaterializedView { name: name.clone() })
            } else {
                Op::DropView(DropView { name: name.clone() })
            });
        }
    }

    for (name, next_proj) in &next_views {
        match prev_views.get(name) {
            None => {
                // New view → CreateView / CreateMaterializedView.
                diff.creates.push(if next_proj.is_materialized {
                    Op::CreateMaterializedView(CreateMaterializedView {
                        name: name.clone(),
                        sql: next_proj.sql.clone(),
                        primary_key: next_proj.primary_key.clone(),
                        source_tables: next_proj.source_tables.clone(),
                    })
                } else {
                    Op::CreateView(CreateView {
                        name: name.clone(),
                        sql: next_proj.sql.clone(),
                        source_tables: next_proj.source_tables.clone(),
                    })
                });
            }
            Some(prev_proj) if prev_proj == next_proj => {
                // Unchanged — no op.
            }
            Some(prev_proj) => {
                // Body changed. Always model as Drop + Create so the
                // ordering works regardless of whether the migration
                // also adds/drops columns the old/new body references
                // — see the `ViewDiff` doc. Loses Postgres atomicity
                // of `CREATE OR REPLACE VIEW`; preserved correctness
                // outweighs the optimization at this stage.
                diff.drops.push(if prev_proj.is_materialized {
                    Op::DropMaterializedView(DropMaterializedView { name: name.clone() })
                } else {
                    Op::DropView(DropView { name: name.clone() })
                });
                diff.creates.push(if next_proj.is_materialized {
                    Op::CreateMaterializedView(CreateMaterializedView {
                        name: name.clone(),
                        sql: next_proj.sql.clone(),
                        primary_key: next_proj.primary_key.clone(),
                        source_tables: next_proj.source_tables.clone(),
                    })
                } else {
                    Op::CreateView(CreateView {
                        name: name.clone(),
                        sql: next_proj.sql.clone(),
                        source_tables: next_proj.source_tables.clone(),
                    })
                });
            }
        }
    }

    diff
}
