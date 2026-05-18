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

use crate::ir::{
    CreateMaterializedView, CreateView, DropMaterializedView, DropView, Op, ReplaceView,
};

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

/// Diff bucket — drops are flushed before table drops, creates after
/// table creates (see `super::diff`).
#[derive(Debug, Default)]
pub(super) struct ViewDiff {
    pub(super) drops: Vec<Op>,
    pub(super) creates: Vec<Op>,
    pub(super) replaces: Vec<Op>,
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
                // Body changed. For non-materialized views Postgres
                // supports `CREATE OR REPLACE VIEW`; the SQLite
                // emitter expands `ReplaceView` to `DROP + CREATE`.
                //
                // Materialized views always require drop + create
                // because Postgres has no `CREATE OR REPLACE
                // MATERIALIZED VIEW`. We model this as a DropMV +
                // CreateMV pair (the drop lands in `diff.drops`, the
                // create in `diff.creates` — already ordered relative
                // to table drops/creates).
                if next_proj.is_materialized || prev_proj.is_materialized {
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
                } else {
                    diff.replaces.push(Op::ReplaceView(ReplaceView {
                        name: name.clone(),
                        sql: next_proj.sql.clone(),
                    }));
                }
            }
        }
    }

    diff
}
